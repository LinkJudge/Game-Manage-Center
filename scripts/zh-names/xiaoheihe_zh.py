#!/usr/bin/env python3
"""按 Steam AppID 从小黑盒补全游戏中文名，写回本地 database.db。

**拟人化版本（v2）**：上一次全量抓取触发了 IP 风控，这版按"真人浏览"的节奏改写。

示例：
  python scripts/zh-names/xiaoheihe_zh.py --dry-run --limit 3      # 先单条验证
  python scripts/zh-names/xiaoheihe_zh.py --scope installed
  python scripts/zh-names/xiaoheihe_zh.py --budget 60              # 一次跑 60 个请求
  python scripts/zh-names/xiaoheihe_zh.py --budget 60 --force      # 忽略冷却标记

拟人化措施（相对 v1 的关键改动）：
1. **会话化**：CookieJar 维持会话，先访问首页与搜索页热身，再打接口
2. **完整浏览器头**：Accept / Accept-Language / Sec-Fetch-* / sec-ch-ua / DNT，
   Referer 带上具体游戏页而非裸域名
3. **随机节奏**：默认 3.5~8.5 秒随机间隔；每 20~38 个请求插入 45~150 秒长停顿
4. **打散顺序**：默认随机洗牌，不再按标题字母序列出请求
5. **单次预算**：--budget 限制本轮请求数（默认 60），到量即优雅退出
6. **早熔断**：**首个 403/429 立即中止**（v1 是连续 6 次），并写下冷却标记，
   6 小时内重跑会直接拒绝（--force 可越过）
7. **软封识别**：接口返回 status 非 ok 且提示"频繁/校验/风控"也算封禁
8. **小批落库**：每 10 条提交一次，中断也不丢已完成的结果

其余行为与 v1 一致：
- 默认只处理 zh_CN 为空、且带 Steam AppID 的记录
- `--record-all`：所有 Steam 记录都检查，已有中文名也把小黑盒名称记入来源表
- API 返回不含中文、或与英文名相同则跳过
- 每个查询结果写入 `game_name_source`（含"无中文名"状态），后续不重复抓取
- 数据库修改前自动备份到 .workbuddy/database-backups
- 可反复运行：成功/无中文名的记录会从待处理集合消失
"""

from __future__ import annotations

import argparse
import gzip
import json
import random
import re
import shutil
import sqlite3
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import winreg
import zlib
from datetime import datetime, timedelta
from http.cookiejar import CookieJar
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DB = ROOT / "src-tauri" / "resources" / "database.db"
BACKUP_DIR = ROOT.parent / ".workbuddy" / "database-backups"
BLOCK_MARKER = ROOT.parent / ".workbuddy" / "zh-names-blocked.json"
API = "https://api.xiaoheihe.cn/game/web/get_game_detail/"
HOME = "https://www.xiaoheihe.cn/"
SEARCH = "https://www.xiaoheihe.cn/search"
CJK = re.compile(r"[\u3400-\u9fff]")
DIGITS = re.compile(r"^\D*(\d+)")

# 浏览器指纹：尽量贴近一次真实的 Chrome 访问
UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
)

BASE_HEADERS = {
    "User-Agent": UA,
    "Accept": "application/json, text/plain, */*",
    "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8",
    "Accept-Encoding": "gzip, deflate",
    "Connection": "keep-alive",
    "DNT": "1",
    "sec-ch-ua": '"Chromium";v="131", "Not_A Brand";v="24", "Google Chrome";v="131"',
    "sec-ch-ua-mobile": "?0",
    "sec-ch-ua-platform": '"Windows"',
    "Sec-Fetch-Dest": "empty",
    "Sec-Fetch-Mode": "cors",
    "Sec-Fetch-Site": "same-site",
}

# 接口返回里出现这些字样，等同于被风控
BLOCK_HINTS = ("频繁", "校验", "风控", "验证", "异常", "forbidden", "limit")


def steam_appid(value: object) -> str | None:
    match = DIGITS.match(str(value or ""))
    return match.group(1) if match else None


def normalize(text: str) -> str:
    return "".join(ch.lower() for ch in text if ch.isalnum())


def has_chinese(text: str) -> bool:
    return bool(CJK.search(text))


class Blocked(Exception):
    """判定为风控封禁，立即中止本轮。"""


class Session:
    """带 Cookie 的会话 + 拟人节奏。"""

    def __init__(self, min_delay: float, max_delay: float, pause_every: tuple[int, int],
                 pause_range: tuple[float, float]) -> None:
        self.jar = CookieJar()
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(self.jar)
        )
        self.min_delay = min_delay
        self.max_delay = max_delay
        self.pause_every = pause_every
        self.pause_range = pause_range
        self.count = 0
        # 本轮长停顿的触发点随机，避免每 N 个固定停一次
        self.next_pause_at = random.randint(*pause_every)

    # ---------- 拟人节奏 ----------

    def breathe(self) -> None:
        """请求之间的间隔；到点则插一次长停顿。"""
        self.count += 1
        if self.count >= self.next_pause_at:
            gap = random.uniform(*self.pause_range)
            print(f"    · 模拟离开（停 {gap:.0f} 秒）……", flush=True)
            time.sleep(gap)
            self.count = 0
            self.next_pause_at = random.randint(*self.pause_every)
        else:
            time.sleep(random.uniform(self.min_delay, self.max_delay))

    # ---------- 请求 ----------

    def _open(self, url: str, headers: dict[str, str], timeout: int) -> bytes:
        request = urllib.request.Request(url, headers=headers)
        try:
            with self.opener.open(request, timeout=timeout) as response:
                raw = response.read()
                encoding = (response.headers.get("Content-Encoding") or "").lower()
        except urllib.error.HTTPError as error:
            if error.code in (403, 429):
                raise Blocked(f"HTTP {error.code}") from error
            raise
        if "gzip" in encoding:
            raw = gzip.decompress(raw)
        elif "deflate" in encoding:
            raw = zlib.decompress(raw)
        return raw

    def warm_up(self) -> None:
        """先像真人一样打开站点，拿到基础 Cookie 再打接口。"""
        for url, accept in (
            (HOME, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
            (SEARCH, "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
        ):
            try:
                self._open(
                    url,
                    {**BASE_HEADERS, "Accept": accept, "Sec-Fetch-Dest": "document",
                     "Sec-Fetch-Mode": "navigate", "Sec-Fetch-Site": "none"},
                    timeout=20,
                )
            except Blocked:
                raise
            except Exception:  # noqa: BLE001 - 热身失败不致命
                pass
            time.sleep(random.uniform(1.5, 3.5))

    def fetch_name(self, appid: str, timeout: int = 20) -> str | None:
        query = urllib.parse.urlencode(
            {"appid": appid, "_time": int(time.time()), "heybox_id": ""}
        )
        headers = {
            **BASE_HEADERS,
            "Referer": f"https://www.xiaoheihe.cn/app/topic/game/pc/{appid}",
            "Origin": "https://www.xiaoheihe.cn",
        }
        raw = self._open(f"{API}?{query}", headers, timeout)
        payload = json.loads(raw.decode("utf-8"))
        status = str(payload.get("status") or "")
        message = str(payload.get("msg") or "")
        if status != "ok":
            if any(hint in message.lower() or hint in message for hint in BLOCK_HINTS):
                raise Blocked(f"status={status} msg={message}")
            return None
        result = payload.get("result") or {}
        return str(result.get("name") or "").strip() or None


def installed_steam_appids() -> set[str]:
    """从 Steam 库读取已安装 AppID，供 --scope installed 使用。"""
    roots: list[Path] = []
    for hive, key, value in [
        (winreg.HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
    ]:
        try:
            with winreg.OpenKey(hive, key) as handle:
                roots.append(Path(winreg.QueryValueEx(handle, value)[0]))
        except OSError:
            pass
    for base in (Path.home() / "AppData/Local/Steam",):
        if base.exists():
            roots.append(base)

    libs: list[Path] = []
    for root in roots:
        if not root.exists():
            continue
        libs.append(root)
        vdf = root / "steamapps" / "libraryfolders.vdf"
        if vdf.exists():
            try:
                text = vdf.read_text(encoding="utf-8", errors="ignore")
            except OSError:
                continue
            libs.extend(
                Path(path.replace("\\\\", "\\"))
                for path in re.findall(r'"path"\s*"([^"]+)"', text)
            )

    appids: set[str] = set()
    seen: set[str] = set()
    for lib in libs:
        key = str(lib).lower()
        if key in seen:
            continue
        seen.add(key)
        for manifest in (lib / "steamapps").glob("appmanifest_*.acf"):
            match = re.search(
                r'"appid"\s*"(\d+)"',
                manifest.read_text(encoding="utf-8", errors="ignore"),
            )
            if match:
                appids.add(match.group(1))
    return appids


def backup_database(db_path: Path) -> Path:
    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    target = BACKUP_DIR / f"{db_path.stem}-xiaoheihe-{datetime.now():%Y%m%d-%H%M%S}.db"
    shutil.copy2(db_path, target)
    return target


def read_block_marker() -> datetime | None:
    try:
        data = json.loads(BLOCK_MARKER.read_text(encoding="utf-8"))
        return datetime.fromisoformat(data["blocked_at"])
    except Exception:  # noqa: BLE001 - 标记缺失或损坏都当作没有
        return None


def write_block_marker(reason: str) -> None:
    BLOCK_MARKER.parent.mkdir(parents=True, exist_ok=True)
    BLOCK_MARKER.write_text(
        json.dumps(
            {"blocked_at": datetime.now().isoformat(timespec="seconds"), "reason": reason},
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="小黑盒 Steam 中文名补全（拟人化版）")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--limit", type=int, default=200, help="最多处理多少条记录；0 = 全部")
    parser.add_argument("--budget", type=int, default=60, help="本轮最多发多少个请求；0 = 不限")
    parser.add_argument("--delay", type=float, default=None, help="固定间隔秒数（默认随机 3.5~8.5）")
    parser.add_argument("--min-delay", type=float, default=3.5)
    parser.add_argument("--max-delay", type=float, default=8.5)
    parser.add_argument("--pause-every", type=int, nargs=2, default=[20, 38],
                        metavar=("MIN", "MAX"), help="每多少个请求插一次长停顿（随机区间）")
    parser.add_argument("--pause-range", type=float, nargs=2, default=[45, 150],
                        metavar=("MIN", "MAX"), help="长停顿秒数区间")
    parser.add_argument("--cooldown-hours", type=float, default=6.0,
                        help="触发封禁后多少小时内拒绝重跑")
    parser.add_argument("--force", action="store_true", help="忽略冷却标记直接运行")
    parser.add_argument("--no-shuffle", action="store_true", help="保持标题顺序（默认随机打散）")
    parser.add_argument("--hot", type=Path, default=None,
                        help="热销榜 JSON（steam_topsellers.py 产出），只抓榜上游戏并按热度排序")
    parser.add_argument("--hot-top", type=int, default=0,
                        help="配合 --hot：只处理榜单前 N 名")
    parser.add_argument("--scope", choices=["all", "installed"], default="all")
    parser.add_argument("--record-all", action="store_true", help="连已有中文名的游戏也检查并记录来源")
    parser.add_argument("--dry-run", action="store_true", help="只预览，不写数据库")
    parser.add_argument("--no-backup", action="store_true")
    args = parser.parse_args()

    if not args.db.exists():
        print(f"[错误] 找不到数据库：{args.db}")
        return 1

    # 冷却检查：上次被封过就别急着再试
    blocked_at = read_block_marker()
    if blocked_at and not args.force:
        wait_until = blocked_at + timedelta(hours=args.cooldown_hours)
        if datetime.now() < wait_until:
            print(
                f"[中止] 上次触发风控于 {blocked_at:%Y-%m-%d %H:%M}，"
                f"冷却到 {wait_until:%Y-%m-%d %H:%M}。确认要跑请加 --force。"
            )
            return 3

    conn = sqlite3.connect(args.db)
    conn.row_factory = sqlite3.Row
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS game_name_source (
            wiki_page_id INTEGER NOT NULL,
            source TEXT NOT NULL,
            name TEXT,
            status TEXT NOT NULL,
            checked_at TEXT NOT NULL,
            PRIMARY KEY (wiki_page_id, source)
        )
        """
    )
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_game_name_source_name ON game_name_source(name)"
    )
    conn.commit()

    extra_condition = "" if args.record_all else "AND (g.zh_CN IS NULL OR trim(g.zh_CN) = '')"
    rows = conn.execute(
        f"""
        SELECT g.wiki_page_id, g.title, g.steam_id, g.zh_CN
        FROM games g
        LEFT JOIN game_name_source n
          ON n.wiki_page_id = g.wiki_page_id AND n.source = 'xiaoheihe'
        WHERE g.steam_id IS NOT NULL
          AND trim(CAST(g.steam_id AS TEXT)) <> ''
          AND n.wiki_page_id IS NULL
          {extra_condition}
        ORDER BY g.title COLLATE NOCASE
        """
    ).fetchall()

    scope_ids = installed_steam_appids() if args.scope == "installed" else None
    if scope_ids is not None:
        if not scope_ids:
            print("[错误] 没有从 Steam 库读到已安装 AppID")
            return 1
        rows = [row for row in rows if steam_appid(row["steam_id"]) in scope_ids]
    if args.hot:
        if not args.hot.exists():
            print(f"[错误] 找不到热销榜文件：{args.hot}")
            return 1
        hot_list = json.loads(args.hot.read_text(encoding="utf-8"))
        rank = {str(item["appid"]): int(item["rank"]) for item in hot_list}
        before = len(rows)
        rows = [row for row in rows if steam_appid(row["steam_id"]) in rank]
        rows.sort(key=lambda row: rank[steam_appid(row["steam_id"])])
        if args.hot_top > 0:
            rows = rows[: args.hot_top]
        print(
            f"热销榜过滤：榜上 {len(rank)} 个 AppID，命中待处理 {len(rows)} 条"
            f"（原待处理 {before} 条），按热度从高到低处理"
        )
    if args.limit > 0:
        rows = rows[: args.limit]
    if not rows:
        print("没有待处理记录。")
        return 0
    if not args.hot and not args.no_shuffle:
        random.shuffle(rows)

    print(f"数据库：{args.db}")
    print(f"待处理：{len(rows)} 条（scope={args.scope}，本轮请求预算 {args.budget or '不限'}）")
    if args.dry_run:
        print("[dry-run] 不写数据库")
    elif not args.no_backup:
        backup = backup_database(args.db)
        print(f"已备份数据库：{backup}")

    min_delay = args.delay if args.delay is not None else args.min_delay
    max_delay = args.delay if args.delay is not None else args.max_delay
    session = Session(
        min_delay=min_delay,
        max_delay=max_delay,
        pause_every=tuple(args.pause_every),
        pause_range=tuple(args.pause_range),
    )
    print("正在模拟浏览器访问站点（热身）……")
    try:
        session.warm_up()
    except Blocked as blocked:
        write_block_marker(str(blocked))
        print(f"[中止] 热身就被拦：{blocked}。已写下冷却标记，{args.cooldown_hours} 小时内重跑会被拒绝。")
        conn.close()
        return 2

    updated = 0
    recorded = 0
    latin = 0
    existing_only = 0
    skipped = 0
    failed = 0
    requests_made = 0
    cache: dict[str, str | None] = {}
    pending: list[tuple[str, int]] = []
    source_pending: list[tuple[int, str, str | None, str, str]] = []

    def flush() -> None:
        if not pending and not source_pending:
            return
        if pending:
            conn.executemany(
                "UPDATE games SET zh_CN = ? WHERE wiki_page_id = ? "
                "AND (zh_CN IS NULL OR trim(zh_CN) = '')",
                pending,
            )
        if source_pending:
            conn.executemany(
                """
                INSERT INTO game_name_source(wiki_page_id, source, name, status, checked_at)
                VALUES(?, ?, ?, ?, ?)
                ON CONFLICT(wiki_page_id, source) DO UPDATE SET
                    name = excluded.name,
                    status = excluded.status,
                    checked_at = excluded.checked_at
                """,
                source_pending,
            )
        conn.commit()
        pending.clear()
        source_pending.clear()

    exit_code = 0
    for index, row in enumerate(rows, 1):
        if args.budget and requests_made >= args.budget:
            print(f"本轮已达请求预算 {args.budget}，收工。下次重跑会从剩余记录继续。")
            break

        appid = steam_appid(row["steam_id"])
        if not appid:
            skipped += 1
            continue

        if appid in cache:
            name = cache[appid]
        else:
            session.breathe()
            requests_made += 1
            try:
                name = session.fetch_name(appid)
            except Blocked as blocked:
                write_block_marker(str(blocked))
                print(f"[{index}/{len(rows)}] {row['title']} ({appid}) -> 触发风控：{blocked}")
                print(
                    f"已立即中止并写入冷却标记（{args.cooldown_hours} 小时内拒绝重跑）。"
                    "本次已完成的数据均已落库。"
                )
                exit_code = 2
                break
            except urllib.error.HTTPError as error:
                failed += 1
                print(f"[{index}/{len(rows)}] {row['title']} ({appid}) -> HTTP {error.code}")
                if error.code >= 500:
                    time.sleep(random.uniform(30, 60))
                continue
            except Exception as error:  # noqa: BLE001 - 单条网络异常只跳过
                failed += 1
                print(f"[{index}/{len(rows)}] {row['title']} ({appid}) -> 失败：{error}")
                time.sleep(random.uniform(5, 12))
                continue
            cache[appid] = name

        checked_at = datetime.now().isoformat(timespec="seconds")

        if not name:
            skipped += 1
            if not args.dry_run:
                source_pending.append(
                    (row["wiki_page_id"], "xiaoheihe", None, "no_name", checked_at)
                )
            print(f"[{index}/{len(rows)}] {row['title']} ({appid}) -> 无中文名，已记录")
        elif not has_chinese(name):
            # 小黑盒有这个游戏，但显示名不含中文（如 CS2、CS:GO）：
            # 只作为来源记录留存供交叉验证，绝不用它覆盖主名
            latin += 1
            if not args.dry_run:
                source_pending.append(
                    (row["wiki_page_id"], "xiaoheihe", name, "latin", checked_at)
                )
            print(f"[{index}/{len(rows)}] {row['title']} ({appid}) -> 非中文名「{name}」，仅记录")
        else:
            recorded += 1
            existing = str(row["zh_CN"] or "").strip()
            if existing and normalize(existing) == normalize(name):
                pass
            elif existing:
                existing_only += 1
                print(
                    f"[{index}/{len(rows)}] {row['title']} -> 记录小黑盒名：{name} "
                    f"（保留现有：{existing}）"
                )
            else:
                updated += 1
                print(f"[{index}/{len(rows)}] {row['title']} -> 补入主名：{name}")
            if not args.dry_run:
                source_pending.append(
                    (row["wiki_page_id"], "xiaoheihe", name, "named", checked_at)
                )
                if not existing:
                    pending.append((name, row["wiki_page_id"]))

        if not args.dry_run and len(source_pending) >= 10:
            flush()

    if not args.dry_run:
        flush()
    conn.close()

    print(
        f"完成：请求 {requests_made} 次，小黑盒中文名记录 {recorded}"
        f"（补入主名 {updated}，已有主名仅记别名 {existing_only}），"
        f"非中文名 {latin}，无名字 {skipped}，失败 {failed}"
    )
    if args.dry_run:
        print("这是预览结果；去掉 --dry-run 才会写库。")
    if exit_code == 2:
        print("任务因风控提前结束；已成功的数据均已落库。")
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
