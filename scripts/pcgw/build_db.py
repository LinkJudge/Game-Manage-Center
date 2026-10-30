#!/usr/bin/env python3
"""把 sync-pcgamingwiki.js 构建好的行数据写成 SQLite 库。

为什么单独一个脚本：
`sync-pcgamingwiki.js` 原来用 node-sqlite3 直写，5.2 万条要跑十几分钟
（实测卡在 2 MB 不动）。原因是在 node-sqlite3 里
`db.run('BEGIN')` 只是把语句排进队列、不保证先于 `stmt.run` 执行，
事务实际没开起来，于是每条 INSERT 都自成一个事务并 fsync 一次；
换成 `db.exec('BEGIN')` 也一样。Python 的 `executemany` 包在一个事务里
几秒就能写完，而且行为可预测。

输入：scripts/pcgw/database/build-rows.json（数组，字段见下）
输出：scripts/pcgw/database/database.new.db

用法：
  node scripts/pcgw/sync-pcgamingwiki.js   # 构建行数据
  python scripts/pcgw/build_db.py          # 写库（本脚本）
  python scripts/db/apply_pcgw_delta.py    # 合并进应用库（保护中文名）
"""

from __future__ import annotations

import json
import sqlite3
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent
ROWS_FILE = ROOT / "database" / "build-rows.json"
OUT_DB = ROOT / "database" / "database.new.db"

SCHEMA = """
CREATE TABLE games (
    title TEXT PRIMARY KEY,
    wiki_page_id INTEGER UNIQUE,
    install_folder TEXT,
    steam_id INTEGER,
    gog_id INTEGER,
    save_location TEXT,
    platform TEXT,
    zh_CN TEXT
);
CREATE TABLE metadata (
    key TEXT PRIMARY KEY,
    value TEXT
);
"""

FIELDS = (
    "title",
    "wiki_page_id",
    "install_folder",
    "steam_id",
    "gog_id",
    "save_location",
    "platform",
    "zh_CN",
)


def main() -> int:
    if not ROWS_FILE.exists():
        print(f"[错误] 找不到行数据：{ROWS_FILE}")
        print("       先跑 node scripts/pcgw/sync-pcgamingwiki.js")
        return 1

    rows = json.loads(ROWS_FILE.read_text(encoding="utf-8"))
    print(f"读入 {len(rows)} 行")

    if OUT_DB.exists():
        OUT_DB.unlink()
        print(f"已删除旧的新库 {OUT_DB.name}")

    conn = sqlite3.connect(OUT_DB)
    try:
        # 大事务一次提交，比逐条提交快两个数量级
        with conn:
            conn.executescript(SCHEMA)
            conn.executemany(
                f"INSERT OR REPLACE INTO games({','.join(FIELDS)}) "
                f"VALUES({','.join('?' * len(FIELDS))})",
                [tuple(r.get(f) for f in FIELDS) for r in rows],
            )
            now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f") + "000"
            conn.executemany(
                "INSERT INTO metadata(key, value) VALUES(?,?)",
                [
                    ("last_recent_change_time", now),
                    ("sync_source", "pcgamingwiki-wikitext"),
                    ("synced_at", now),
                ],
            )
    finally:
        conn.close()

    check = sqlite3.connect(f"file:{OUT_DB}?mode=ro", uri=True)
    try:
        total = check.execute("SELECT COUNT(*) FROM games").fetchone()[0]
        named = check.execute(
            "SELECT COUNT(*) FROM games WHERE zh_CN IS NOT NULL AND trim(zh_CN) <> ''"
        ).fetchone()[0]
        withsave = check.execute(
            "SELECT COUNT(*) FROM games WHERE save_location NOT IN ('{}','')"
        ).fetchone()[0]
    finally:
        check.close()

    print(f"已写入 {OUT_DB}")
    print(f"  行数 {total}，带 zh_CN {named}，带存档数据 {withsave}")
    print("下一步：python scripts/db/apply_pcgw_delta.py --dry-run")
    return 0


if __name__ == "__main__":
    sys.exit(main())
