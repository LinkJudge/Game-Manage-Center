"""盘点标题字段的实际构成：name_zh 与 name_en 分别有哪些"文字种类"。"""
import re
import sqlite3
from pathlib import Path

DB = Path(__file__).resolve().parents[2] / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"

CJK = re.compile(r"[\u4e00-\u9fff]")
KANA = re.compile(r"[\u3040-\u30ff]")
HANGUL = re.compile(r"[\uac00-\ud7af]")
CYRILLIC = re.compile(r"[\u0400-\u04ff]")
LAT = re.compile(r"[A-Za-z]")


def kind(s: str) -> str:
    s = (s or "").strip()
    if not s:
        return "空"
    has_cjk, has_kana = bool(CJK.search(s)), bool(KANA.search(s))
    has_lat = bool(LAT.search(s))
    if KANA.search(s) and not has_cjk:
        return "纯假名（日文）"
    if has_kana:
        return "汉字+假名（日文）"
    if HANGUL.search(s):
        return "韩文"
    if CYRILLIC.search(s):
        return "西里尔/俄文"
    if has_cjk and has_lat:
        return "中英混合"
    if has_cjk:
        return "纯中文"
    if has_lat:
        return "纯拉丁字母"
    return "仅数字/符号"


def table(rows, label):
    from collections import Counter

    c = Counter(kind(r) for r in rows)
    total = len(rows)
    print(f"\n=== {label}（共 {total:,} 条）===")
    for k, v in c.most_common():
        print(f"  {k:18s} {v:6,}  {v / total * 100:5.1f}%")
    return c


conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
try:
    zh = [r[0] for r in conn.execute("SELECT name_zh FROM games")]
    en = [r[0] for r in conn.execute("SELECT name_en FROM games")]
    total = conn.execute("SELECT COUNT(*) FROM games").fetchone()[0]
    # name_en 里带汉字的样本
    bad_en = conn.execute(
        "SELECT pcgw_id, name_en, name_zh FROM games "
        "WHERE name_en LIKE '%' || char(0x4e00) || '%'"
    ).fetchall()
    kana_en = conn.execute(
        "SELECT pcgw_id, name_en FROM games WHERE name_en GLOB '*[ぁ-ん]*' OR name_en GLOB '*[ァ-ヶ]*'"
    ).fetchall()
    empty_en = conn.execute(
        "SELECT COUNT(*) FROM games WHERE name_en IS NULL OR trim(name_en)=''"
    ).fetchone()[0]
    empty_zh = conn.execute(
        "SELECT COUNT(*) FROM games WHERE name_zh IS NULL OR trim(name_zh)=''"
    ).fetchone()[0]
    # 来源分布
    srcs = conn.execute(
        "SELECT source, COUNT(*) FROM name_sources GROUP BY source ORDER BY 2 DESC"
    ).fetchall()
finally:
    conn.close()

print(f"games 总数 {total:,}")
table(zh, "name_zh（UI 主标题用的字段）")
table(en, "name_en（英文名字段）")
print(f"\nname_zh 空: {empty_zh:,}    name_en 空: {empty_en:,}")
print("\n=== name_en 含假名的样本 ===")
for pid, s in kana_en[:6]:
    print(f"  [{pid}] {s!r}")
print("\n=== name_sources 来源分布 ===")
for s, n in srcs:
    print(f"  {s:16s} {n:,}")
