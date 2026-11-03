"""采集目标、单进程锁和共享日预算。导入本模块不会联网或写库。"""
from __future__ import annotations
import json
import os
import sqlite3
from contextlib import contextmanager
from datetime import datetime, timedelta
from pathlib import Path


@contextmanager
def connection(*args, **kwargs):
    conn = sqlite3.connect(*args, **kwargs)
    try:
        with conn:
            yield conn
    finally:
        conn.close()

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DB = ROOT / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"
STATE_DIR = ROOT.parent / ".workbuddy"
DAILY_LIMIT = 3000

class BudgetExceeded(RuntimeError):
    """当天请求预算耗尽，必须停止整轮。"""


def validate_target(path: Path) -> Path:
    path = path.resolve()
    forbidden = { (ROOT / "src-tauri/resources/database.db").resolve() }
    if os.environ.get("APPDATA"):
        forbidden.add((Path(os.environ["APPDATA"]) / "com.gamemanagecenter.app/database.db").resolve())
    if path in forbidden:
        raise ValueError("采集必须写开发全量库，不能写发布库或应用运行库")
    if not path.is_file():
        raise ValueError(f"开发全量库不存在：{path}")
    with connection(path.as_uri() + "?mode=ro", uri=True) as conn:
        conn.execute("SELECT pcgw_id, name_en, name_zh, steam_appid FROM games LIMIT 0")
    return path

def ensure_raw(conn: sqlite3.Connection, source: str) -> None:
    table, key = ("xhh_raw", "pcgw_id") if source == "xiaoheihe" else ("steam_raw", "steam_appid")
    key_type = "INTEGER" if source == "xiaoheihe" else "TEXT"
    conn.execute(f"CREATE TABLE IF NOT EXISTS {table} ({key} {key_type} PRIMARY KEY, payload BLOB NOT NULL, fetched_at TEXT NOT NULL)")

@contextmanager
def single_collector(lock_path: Path | None = None):
    """Windows 内核文件锁，进程退出自动释放；主采集与库外补采共用。"""
    path = lock_path or STATE_DIR / "xhh-collection.lock"
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+b") as handle:
        if handle.tell() == 0:
            handle.write(b"0"); handle.flush()
        handle.seek(0)
        try:
            if os.name == "nt":
                import msvcrt
                msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except (OSError, BlockingIOError) as error:
            raise RuntimeError("已有小黑盒采集进程运行，本轮未启动") from error
        try:
            yield
        finally:
            handle.seek(0)
            if os.name == "nt":
                import msvcrt
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                import fcntl
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)

def daily_usage(conn: sqlite3.Connection, day: str | None = None) -> int:
    day = day or datetime.now().date().isoformat()
    # 首次启用账本时，用历史采集记录作保守基线；之后逐请求累计。
    tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    if "collection_request_days" in tables:
        row = conn.execute("SELECT baseline, requests FROM collection_request_days WHERE day=?", (day,)).fetchone()
        if row:
            return int(row[0]) + int(row[1])
    count = conn.execute("SELECT COUNT(*) FROM name_sources WHERE source='xiaoheihe' AND checked_at>=? AND checked_at<?", (day, (datetime.fromisoformat(day) + timedelta(days=1)).date().isoformat())).fetchone()[0]
    if "extra_names" in tables:
        count += conn.execute("SELECT COUNT(*) FROM extra_names WHERE checked_at>=? AND checked_at<?", (day, (datetime.fromisoformat(day) + timedelta(days=1)).date().isoformat())).fetchone()[0]
    return count

def reserve_request(db: Path, count: int = 1, limit: int = DAILY_LIMIT) -> None:
    """发请求之前独立事务记账，失败请求也消耗预算；重启后仍有效。"""
    day = datetime.now().date().isoformat()
    with connection(db, timeout=5) as conn:
        conn.execute("BEGIN IMMEDIATE")
        used = daily_usage(conn, day)
        if used + count > limit:
            raise BudgetExceeded(f"当天小黑盒请求预算已用 {used}/{limit}，本轮停止，等待次日")
        conn.execute("CREATE TABLE IF NOT EXISTS collection_request_days(day TEXT PRIMARY KEY, baseline INTEGER NOT NULL, requests INTEGER NOT NULL)")
        conn.execute("INSERT INTO collection_request_days(day,baseline,requests) VALUES(?,?,?) ON CONFLICT(day) DO UPDATE SET requests=requests+excluded.requests", (day, used, count))

def preflight(path: Path) -> dict:
    path = validate_target(path)
    with connection(path.as_uri() + "?mode=ro", uri=True) as conn:
        used = daily_usage(conn)
        tables = {r[0] for r in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    marker = STATE_DIR / "zh-names-blocked.json"
    return {"database": str(path), "requests_today": used, "daily_limit": DAILY_LIMIT,
            "budget_available": used < DAILY_LIMIT, "raw_tables": sorted(tables & {"xhh_raw", "steam_raw"}),
            "blocked": json.loads(marker.read_text(encoding="utf-8")) if marker.exists() else None}


def collector_running() -> bool:
    path = STATE_DIR / "xhh-collection.lock"
    if not path.exists():
        return False
    try:
        with single_collector(path):
            return False
    except RuntimeError:
        return True
