"""检查 games.name_zh_hant 是否混入简体（用 zhconv 反推：能转成不同繁体 = 原文含简体）。"""

import sqlite3

from zhconv import convert

c = sqlite3.connect("file:src-tauri/resources/database.db?mode=ro", uri=True)
rows = c.execute(
    "SELECT name_zh, name_zh_hant FROM games "
    "WHERE name_zh_hant IS NOT NULL AND trim(name_zh_hant) <> ''"
).fetchall()

same_as_zh = 0
looks_simplified = []
for zh, hant in rows:
    if zh and zh.strip() == hant.strip():
        same_as_zh += 1
    if convert(hant, "zh-tw") != hant:
        looks_simplified.append(hant)

print(f"name_zh_hant 非空：{len(rows)} 条")
print(f"  与 name_zh 完全相同：{same_as_zh}")
print(f"  其实仍是简体（转繁体后不一样）：{len(looks_simplified)}")
for s in looks_simplified[:8]:
    print(f"    {s}  →  {convert(s, 'zh-tw')}")
c.close()
