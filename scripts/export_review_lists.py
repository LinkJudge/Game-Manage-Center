"""生成四份审查清单（③ 冲突 / ④ 重复页 / ⑤ zh==en / ⑥ 繁中缺失）。

小黑盒采集正在写库，这里全部走只读连接，不抢锁。
输出：docs/review/*.csv（UTF-8 BOM，Excel 直接打开不乱码）。
"""

from __future__ import annotations

import csv
import re
import sqlite3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DB = ROOT / "src-tauri" / "resources" / "database.db"
OUT = ROOT / "docs" / "review"
CJK = re.compile(r"[\u4e00-\u9fff]")
KANA = re.compile(r"[\u3040-\u30ff]")


def connect() -> sqlite3.Connection:
    conn = sqlite3.connect(f"file:{DB.as_posix()}?mode=ro", uri=True, timeout=30)
    conn.execute("PRAGMA busy_timeout=30000")
    return conn


def write_csv(name: str, header: list[str], rows: list[list]) -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    path = OUT / name
    with open(path, "w", newline="", encoding="utf-8-sig") as f:
        w = csv.writer(f)
        w.writerow(header)
        w.writerows(rows)
    print(f"  {name}: {len(rows)} 行")
    return len(rows)


def main() -> int:
    c = connect()

    # ---------- ③ 两源译名冲突 ----------
    rows = c.execute(
        """
        SELECT g.name_en, s.name_zh, n.name
        FROM games g
        JOIN steam_names s
          ON CAST(s.steam_appid AS TEXT) = CAST(g.steam_appid AS TEXT)
         AND trim(s.name_zh) <> ''
        JOIN name_sources n
          ON n.pcgw_id = g.pcgw_id AND n.source = 'xiaoheihe' AND trim(n.name) <> ''
        WHERE s.name_zh <> n.name
        ORDER BY g.name_en
        """
    ).fetchall()
    out_rows = []
    for en, steam, xhh in rows:
        st, xh = steam.strip(), xhh.strip()
        if xh in st or st in xh:
            kind = "C-包含关系(建议选干净一侧)"
        elif not CJK.search(st):
            kind = "A-Steam无中文(选小黑盒合理)"
        else:
            kind = "B-译名不同(需人工裁决)"
        out_rows.append([en, st, xh, "小黑盒(现状)", kind])
    n3 = write_csv("conflicts.csv", ["英文名", "Steam官方名", "小黑盒名", "当前采用", "类别建议"], out_rows)

    # ---------- ④ 同 AppID 重复页 ----------
    dups = c.execute(
        "SELECT steam_appid, COUNT(*) n FROM games WHERE steam_appid > 0 "
        "GROUP BY steam_appid HAVING n > 1"
    ).fetchall()
    out_rows = []
    for appid, _n in dups:
        rows = c.execute(
            "SELECT pcgw_id, name_en, save_paths FROM games "
            "WHERE steam_appid = ? ORDER BY pcgw_id",
            (appid,),
        ).fetchall()
        out_rows.append(
            [
                appid,
                len(rows),
                " / ".join(str(r[0]) for r in rows),
                " | ".join((r[1] or "")[:28] for r in rows),
                " || ".join((r[2] or "")[:40].replace("\n", " ") for r in rows),
            ]
        )
    n4 = write_csv(
        "duplicate_appids.csv",
        ["steam_appid", "行数", "pcgw_id列表", "各行英文名", "各行存档路径(截断)"],
        out_rows,
    )

    # ---------- ⑤ name_zh == name_en ----------
    rows = c.execute(
        "SELECT name_en, steam_appid, pcgw_id FROM games "
        "WHERE name_zh IS NOT NULL AND trim(name_zh) <> '' "
        "AND lower(name_zh) = lower(name_en) ORDER BY name_en"
    ).fetchall()
    out_rows = []
    for en, appid, pcgw in rows:
        kana, han = bool(KANA.search(en)), bool(CJK.search(en))
        if kana:
            sug = "含假名-建议清空(假名不是中文)"
        elif han:
            sug = "中英混排-保留"
        else:
            sug = "纯拉丁-异常(人工看)"
        out_rows.append([en, appid or "", pcgw or "", "是" if kana else "否", sug])
    n5 = write_csv(
        "zh_eq_en.csv",
        ["英文名(=中文名)", "steam_appid", "pcgw_id", "含假名", "判定建议"],
        out_rows,
    )

    # ---------- ⑥ 繁中缺失 24 条：其它语言是否采到 ----------
    rows = c.execute(
        "SELECT steam_appid, name_en, name_zh, name_ja FROM steam_names "
        "WHERE status IN ('ok','en_only') AND name_zh_hant IS NULL ORDER BY steam_appid"
    ).fetchall()
    out_rows = []
    for appid, en, zh, ja in rows:
        if (ja or "").strip():
            verdict = "日文有→未下架，确实无繁中"
        elif (zh or "").strip():
            verdict = "简体有→未下架，确实无繁中"
        else:
            verdict = "其它语言也没有→疑似下架/接口失效"
        out_rows.append([appid, en, (zh or "")[:30], (ja or "")[:30], verdict])
    n6 = write_csv(
        "hant_missing.csv",
        ["steam_appid", "英文名", "简体名", "日文名", "判定"],
        out_rows,
    )

    c.close()
    print(f"\n输出目录：{OUT}")
    print(f"汇总：冲突 {n3} / 重复组 {n4} / zh==en {n5} / 繁中缺失 {n6}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
