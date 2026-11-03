"""全表审计：找出各名字表里"中英字段串味"的条目。

检查项：
  games.name_en 含汉字       ← 英文名字段里出现中文
  games.name_zh 纯拉丁       ← 中文字段里只有英文
  steam_names.name_en 含汉字
  steam_names.name_zh 纯拉丁（status=ok 时不该出现）
  name_sources.name 与 status 不一致（latin 却含汉字 / named 却无汉字）
  extra_names 同上
"""

from __future__ import annotations

import re
import sqlite3
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DB = ROOT / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"

CJK = re.compile(r"[\u4e00-\u9fff]")
LAT = re.compile(r"[A-Za-z]")


def main() -> int:
    conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        def scan(label: str, sql: str, col: int, check, expect: str) -> None:
            bad = [row for row in conn.execute(sql) if check(row[col] or "")]
            print(f"{label:34s} {len(bad):6d}")
            for row in bad[:3]:
                print(f"    {row}")

        print("=== 字段串味审计 ===\n")
        scan(
            "games.name_en 含汉字",
            "SELECT pcgw_id, steam_appid, name_en FROM games",
            2,
            lambda s: bool(CJK.search(s)),
            "应为 0 或仅官方无英文名的情况",
        )
        scan(
            "games.name_zh 纯拉丁（无汉字）",
            "SELECT pcgw_id, name_zh FROM games WHERE name_zh IS NOT NULL AND trim(name_zh) <> ''",
            1,
            lambda s: not CJK.search(s) and bool(LAT.search(s)),
            "应为 0",
        )
        scan(
            "steam_names.name_en 含汉字",
            "SELECT steam_appid, name_zh, name_en FROM steam_names",
            2,
            lambda s: bool(CJK.search(s)),
            "应为 0",
        )
        scan(
            "steam_names.status=ok 但 name_zh 无汉字",
            "SELECT steam_appid, name_zh FROM steam_names WHERE status='ok'",
            1,
            lambda s: not CJK.search(s),
            "应为 0",
        )
        scan(
            "steam_names.status=en_only 但 name_en 含汉字",
            "SELECT steam_appid, name_en FROM steam_names WHERE status='en_only'",
            1,
            lambda s: bool(CJK.search(s)),
            "应为 0",
        )
        scan(
            "name_sources status=latin 却含汉字",
            "SELECT pcgw_id, status, name FROM name_sources WHERE status='latin'",
            2,
            lambda s: bool(CJK.search(s)),
            "应为 0",
        )
        scan(
            "name_sources status=named 却无汉字",
            "SELECT pcgw_id, status, name FROM name_sources WHERE status='named'",
            2,
            lambda s: not CJK.search(s),
            "应为 0",
        )
        scan(
            "extra_names status=latin 却含汉字",
            "SELECT steam_appid, status, name FROM extra_names WHERE status='latin'",
            2,
            lambda s: bool(CJK.search(s)),
            "应为 0",
        )
        scan(
            "extra_names status=named 却无汉字",
            "SELECT steam_appid, status, name FROM extra_names WHERE status='named'",
            2,
            lambda s: not CJK.search(s),
            "应为 0",
        )
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
