"""数据现状快照：一张表看清名字覆盖率、raw 覆盖、各来源进度。

用法：python scripts/zh-names/snapshot.py
"""

import sqlite3
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DB = ROOT / "src-tauri" / "resources" / "database.db"


def main() -> int:
    c = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    q = lambda sql: c.execute(sql).fetchone()[0]  # noqa: E731

    total = q("SELECT COUNT(*) FROM games")
    zh = q("SELECT COUNT(*) FROM games WHERE name_zh IS NOT NULL AND trim(name_zh) <> ''")
    en = q("SELECT COUNT(*) FROM games WHERE name_en IS NOT NULL AND trim(name_en) <> ''")
    hant = q("SELECT COUNT(*) FROM games WHERE name_zh_hant IS NOT NULL AND trim(name_zh_hant) <> ''")
    no_appid = q("SELECT COUNT(*) FROM games WHERE steam_appid IS NULL OR trim(CAST(steam_appid AS TEXT)) = ''")

    print(f"games 总数               {total:,}")
    print(f"  name_zh 非空           {zh:,}  ({zh / total * 100:.1f}%)")
    print(f"  name_en 非空           {en:,}  ({en / total * 100:.1f}%)")
    print(f"  name_zh_hant 非空      {hant:,}  ({hant / total * 100:.1f}%)")
    print(f"  没有 steam_appid       {no_appid:,}")

    print("\nsteam_names 状态：")
    for status, count in c.execute(
        "SELECT status, COUNT(*) FROM steam_names GROUP BY status ORDER BY 2 DESC"
    ):
        print(f"  {status:10s} {count:7,}")

    print("\n原始响应（gzip）：")
    for table in ("steam_raw", "xhh_raw"):
        rows, size = c.execute(f"SELECT COUNT(*), SUM(LENGTH(payload)) FROM {table}").fetchone()
        size = size or 0
        print(f"  {table:10s} {rows:7,} 行  {size / 1024 / 1024:.1f} MB")

    print("\n小黑盒：")
    total_src = q("SELECT COUNT(*) FROM name_sources")
    named = q("SELECT COUNT(*) FROM name_sources WHERE status='named'")
    latin = q("SELECT COUNT(*) FROM name_sources WHERE status='latin'")
    none_name = q("SELECT COUNT(*) FROM name_sources WHERE status='no_name'")
    pending = q(
        "SELECT COUNT(*) FROM games g "
        "LEFT JOIN name_sources s ON s.pcgw_id = g.pcgw_id AND s.source = 'xiaoheihe' "
        "WHERE g.steam_appid IS NOT NULL AND trim(CAST(g.steam_appid AS TEXT)) <> '' "
        "AND s.pcgw_id IS NULL"
    )
    print(f"  已采 {total_src:,}（有中文 {named:,} / 无中文 {latin:,} / 查无 {none_name:,}）")
    print(f"  待采 {pending:,}")

    fake_zh = q(
        "SELECT COUNT(*) FROM steam_names WHERE status='en_only' "
        "AND name_zh IS NOT NULL AND trim(name_zh) <> ''"
    )
    print(f"\n遗留清理项：en_only 里的假 name_zh（英文串）{fake_zh:,} 条")
    c.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
