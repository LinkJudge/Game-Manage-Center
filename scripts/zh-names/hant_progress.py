"""读当前繁中采集进度（供 shell 轮询用）。"""

import sqlite3
from pathlib import Path

DB = Path(__file__).resolve().parents[2] / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"
c = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
hant = c.execute(
    "SELECT COUNT(*) FROM steam_names WHERE name_zh_hant IS NOT NULL AND trim(name_zh_hant) <> ''"
).fetchone()[0]
none_found = c.execute("SELECT COUNT(*) FROM steam_names WHERE name_zh_hant = ''").fetchone()[0]
pending = c.execute(
    "SELECT COUNT(*) FROM steam_names WHERE status IN ('ok','en_only') AND name_zh_hant IS NULL"
).fetchone()[0]
c.close()
print(f"hant={hant} checked_none={none_found} pending={pending}")
