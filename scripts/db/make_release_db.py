#!/usr/bin/env python3
"""从开发全量库导出发布快照：保留原库，裁 raw，兼容 0.3/0.4 读取。"""
from __future__ import annotations
from contextlib import contextmanager
import argparse
import json
import os
import sqlite3
import tempfile
from datetime import datetime
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
DEFAULT_SOURCE = ROOT / "scripts/pcgw/database/gmc-dev-full.db"
DEFAULT_OUT = ROOT / "src-tauri/resources/database.db"
REQUIRED = ("pcgw_id","name_en","name_zh","name_zh_hant","install_dir","steam_appid","gog_appid","save_paths","stores")
LEGACY = {"title":"name_en","wiki_page_id":"pcgw_id","install_folder":"install_dir",
          "steam_id":"steam_appid","gog_id":"gog_appid","platform":"stores","zh_CN":"name_zh","save_location":"save_paths"}

def validate(conn: sqlite3.Connection, legacy: bool = False) -> dict:
    if conn.execute("PRAGMA quick_check(1)").fetchone()[0] != "ok":
        raise ValueError("SQLite 完整性检查失败")
    conn.execute("SELECT " + ",".join(REQUIRED) + " FROM games LIMIT 1").fetchall()
    count = conn.execute("SELECT COUNT(*) FROM games").fetchone()[0]
    if not count:
        raise ValueError("拒绝导出空数据库")
    if legacy:
        conn.execute("SELECT " + ",".join(LEGACY) + " FROM games LIMIT 1").fetchall()
    return {"games":count, "zh":conn.execute("SELECT COUNT(*) FROM games WHERE trim(coalesce(name_zh,''))<>''").fetchone()[0],
            "hant":conn.execute("SELECT COUNT(*) FROM games WHERE trim(coalesce(name_zh_hant,''))<>''").fetchone()[0]}

def backup_file(source: Path, destination: Path) -> None:
    with connection(source.as_uri()+"?mode=ro", uri=True) as src, connection(destination) as dst:
        src.backup(dst)

def export_database(source: Path, out: Path, backup_dir: Path | None = None) -> dict:
    source, out = source.resolve(), out.resolve()
    if source == out:
        raise ValueError("源库与输出库不能相同")
    if not source.is_file():
        raise ValueError("开发全量库不存在，请先准备开发库或下载发布库用于构建")
    out.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=out.name+".", suffix=".tmp", dir=out.parent)
    os.close(fd)
    tmp = Path(temporary)
    try:
        # 保留写锁直到 SQLite backup 完成；有采集写入者时直接报错。
        with connection(source, timeout=0) as guard:
            guard.execute("BEGIN IMMEDIATE")
            with connection(source.as_uri()+"?mode=ro",uri=True) as src, connection(tmp) as dst:
                expected = validate(src)
                src.backup(dst)
            guard.rollback()
        with connection(tmp) as conn:
            for table in ("steam_raw","xhh_raw","xhh_extra_raw","collection_request_days"):
                conn.execute(f"DROP TABLE IF EXISTS {table}")
            cols = {r[1].lower() for r in conn.execute("PRAGMA table_xinfo(games)")}
            for alias, origin in LEGACY.items():
                if alias.lower() not in cols:
                    conn.execute(f'ALTER TABLE games ADD COLUMN "{alias}" GENERATED ALWAYS AS ("{origin}") VIRTUAL')
            conn.commit()
            conn.execute("PRAGMA journal_mode=DELETE")
            conn.execute("VACUUM")
            actual = validate(conn, legacy=True)
            if actual != expected:
                raise ValueError("导出前后游戏或名称数量变化，未替换输出")
        if out.exists():
            backup_dir = backup_dir or ROOT.parent/".workbuddy-backup/release-db"
            backup_dir.mkdir(parents=True,exist_ok=True)
            backup_file(out, backup_dir/(out.name+"."+datetime.now().strftime("%Y%m%d-%H%M%S-%f")+".bak"))
        os.replace(tmp,out)
        return {**actual,"output":str(out),"bytes":out.stat().st_size,"legacy_compatibility":"0.3/0.4"}
    finally:
        tmp.unlink(missing_ok=True)

def main() -> int:
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source",type=Path,default=DEFAULT_SOURCE)
    parser.add_argument("--out",type=Path,default=DEFAULT_OUT)
    parser.add_argument("--check",action="store_true",help="只读验证输出，不导出、不联网")
    args=parser.parse_args()
    try:
        if args.check:
            with connection(args.out.resolve().as_uri()+"?mode=ro",uri=True) as conn:
                result=validate(conn,legacy=True)
        else:
            result=export_database(args.source,args.out)
        print(json.dumps(result,ensure_ascii=False,indent=2))
        return 0
    except (OSError,ValueError,sqlite3.Error) as error:
        print(f"导出失败：{error}")
        return 1
if __name__=="__main__":
    raise SystemExit(main())
