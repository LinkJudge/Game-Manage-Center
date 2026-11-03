"""对比库里现有 name_en 与 Steam 官方 name_en，量化"切换数据源"会改多少行。

用 Python 做内存比对，避免在 SQL 里做 4.9 万行 join（会卡住）。
"""

import sqlite3
from pathlib import Path

DB = Path(__file__).resolve().parents[2] / "src-tauri" / "resources" / "database.db"
c = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)

games = c.execute("SELECT pcgw_id, steam_appid, name_en FROM games").fetchall()
official = {
    str(appid): name
    for appid, name in c.execute(
        "SELECT steam_appid, name_en FROM steam_names "
        "WHERE name_en IS NOT NULL AND trim(name_en) <> ''"
    )
}
c.close()

total = len(games)
en_nonempty = sum(1 for _, _, en in games if en and en.strip())
comparable = same = diff = 0
samples = []
for pcgw_id, appid, name_en in games:
    if appid is None:
        continue
    key = str(appid)
    if key not in official:
        continue
    comparable += 1
    current = (name_en or "").strip()
    target = official[key].strip()
    if current == target:
        same += 1
    else:
        diff += 1
        if len(samples) < 10:
            samples.append((current, target))

print(f"games 总数              {total:,}")
print(f"name_en 非空            {en_nonempty:,}")
print()
print(f"能和官方英文名对上的     {comparable:,}")
print(f"  完全相同              {same:,}")
print(f"  不同（切换会改这些）    {diff:,}")
print()
print("抽样：")
for wiki, off in samples:
    print(f"  wiki → {wiki!r}")
    print(f"  官方 → {off!r}")
