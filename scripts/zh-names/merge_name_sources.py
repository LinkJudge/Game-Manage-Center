"""名字来源合并：把 steam_names / name_sources 归一到 name_entries，
再按优先级把 games.name_zh / name_en / name_zh_hant 重建一遍。

优先级（沿用 docs/zh-name-quality.md 定下的规则）：
  name_zh      : 小黑盒主标题(含汉字) > Steam(含汉字) > 保留原值
  name_en      : Steam 官方 > 小黑盒 name_en > 保留原值(PCGW 标题)
  name_zh_hant : Steam 官方 l=tchinese

用法：
  python scripts/zh-names/merge_name_sources.py          # 只出报告（默认）
  python scripts/zh-names/merge_name_sources.py --apply  # 真正写库
"""

from __future__ import annotations

import argparse
import re
import sqlite3
import sys

from zhconv import convert
from pathlib import Path

DB = Path(__file__).resolve().parents[2] / "src-tauri" / "resources" / "database.db"
CJK = re.compile(r"[\u4e00-\u9fff]")

SCHEMA = """
CREATE TABLE IF NOT EXISTS name_entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    key_kind    TEXT NOT NULL,          -- steam | pcgw
    key_value   TEXT NOT NULL,
    source      TEXT NOT NULL,          -- steam | xiaoheihe
    name_zh     TEXT,
    name_en     TEXT,
    name_zh_hant TEXT,
    status      TEXT,
    checked_at  TEXT,
    UNIQUE (key_kind, key_value, source)
);
CREATE INDEX IF NOT EXISTS idx_name_entries_key ON name_entries (key_kind, key_value);
"""


def connect() -> sqlite3.Connection:
    conn = sqlite3.connect(f"file:{DB.as_posix()}?mode=rw", uri=True, timeout=30)
    conn.execute("PRAGMA busy_timeout=30000")
    return conn


def text(v: str | None) -> str:
    return (v or "").strip()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true", help="真正写库（默认只出报告）")
    args = ap.parse_args()

    conn = connect()
    conn.executescript(SCHEMA)

    # ---------- 1. 迁移到 name_entries ----------
    steam_rows = conn.execute(
        "SELECT steam_appid, name_zh, name_en, name_zh_hant, status, checked_at FROM steam_names"
    ).fetchall()
    xhh_rows = conn.execute(
        "SELECT pcgw_id, source, name, name_en, status, checked_at FROM name_sources"
    ).fetchall()

    conn.executemany(
        """INSERT INTO name_entries (key_kind, key_value, source, name_zh, name_en, name_zh_hant, status, checked_at)
           VALUES ('steam', ?, 'steam', ?, ?, ?, ?, ?)
           ON CONFLICT(key_kind, key_value, source) DO UPDATE SET
             name_zh=excluded.name_zh, name_en=excluded.name_en,
             name_zh_hant=excluded.name_zh_hant, status=excluded.status, checked_at=excluded.checked_at""",
        [(str(a), z, e, h, s, t) for a, z, e, h, s, t in steam_rows],
    )
    conn.executemany(
        """INSERT INTO name_entries (key_kind, key_value, source, name_zh, name_en, status, checked_at)
           VALUES ('pcgw', ?, ?, ?, ?, ?, ?)
           ON CONFLICT(key_kind, key_value, source) DO UPDATE SET
             name_zh=excluded.name_zh, name_en=excluded.name_en,
             status=excluded.status, checked_at=excluded.checked_at""",
        [(str(p), src, z, e, st, t) for p, src, z, e, st, t in xhh_rows],
    )
    conn.commit()
    print(f"[1] 迁移完成：steam {len(steam_rows):,} 行 + 小黑盒 {len(xhh_rows):,} 行 "
          f"→ name_entries {conn.execute('SELECT COUNT(*) FROM name_entries').fetchone()[0]:,} 行")

    # ---------- 2. 建立索引映射 ----------
    games = conn.execute(
        "SELECT id, pcgw_id, steam_appid, name_en, name_zh, name_zh_hant FROM games"
    ).fetchall()
    by_steam: dict[str, list[tuple]] = {}
    by_pcgw: dict[str, list[tuple]] = {}
    for kind, key, source, z, e, h, st in conn.execute(
        "SELECT key_kind, key_value, source, name_zh, name_en, name_zh_hant, status FROM name_entries"
    ):
        (by_steam if kind == "steam" else by_pcgw).setdefault(key, []).append(
            (source, text(z), text(e), text(h), st)
        )

    # ---------- 3. 逐游戏解析 ----------
    stats = {
        "no_source": 0, "zh_from_xhh": 0, "zh_from_steam": 0, "zh_kept": 0,
        "en_from_steam": 0, "en_from_xhh": 0, "en_kept": 0, "hant_from_steam": 0,
        "conflict": 0, "junk_zh": 0, "junk_hant": 0, "zh_eq_en": 0,
    }
    conflicts: list[tuple] = []
    updates: list[tuple] = []

    for gid, pcgw_id, steam_appid, cur_en, cur_zh, cur_hant in games:
        s_entries = by_steam.get(str(steam_appid), []) if steam_appid is not None else []
        p_entries = by_pcgw.get(str(pcgw_id), []) if pcgw_id is not None else []
        if not s_entries and not p_entries:
            stats["no_source"] += 1

        xhh_zh = next((z for src, z, _, _, _ in p_entries if src == "xiaoheihe" and CJK.search(z)), "")
        steam_zh = next((z for src, z, _, _, st in s_entries if CJK.search(z)), "")
        steam_en = next((e for _, _, e, _, _ in s_entries if e), "")
        xhh_en = next((e for src, _, e, _, _ in p_entries if src == "xiaoheihe" and e), "")
        # Steam 的 l=tchinese 对没有繁体版的游戏会回退成简体名（实测 798/3130）。
        # 用 zhconv 反推：如果还能转成"不同的繁体"，说明原文是简体，不算真繁中。
        hant = next(
            (h for _, _, _, h, _ in s_entries
             if CJK.search(h) and convert(h, "zh-tw") == h),
            "",
        )

        if xhh_zh:
            new_zh, stats["zh_from_xhh"] = xhh_zh, stats["zh_from_xhh"] + 1
        elif steam_zh:
            new_zh, stats["zh_from_steam"] = steam_zh, stats["zh_from_steam"] + 1
        else:
            new_zh, stats["zh_kept"] = text(cur_zh), stats["zh_kept"] + 1

        if steam_en:
            new_en, stats["en_from_steam"] = steam_en, stats["en_from_steam"] + 1
        elif xhh_en:
            new_en, stats["en_from_xhh"] = xhh_en, stats["en_from_xhh"] + 1
        else:
            new_en, stats["en_kept"] = text(cur_en), stats["en_kept"] + 1

        new_hant = hant
        if hant:
            stats["hant_from_steam"] += 1

        if xhh_zh and steam_zh and xhh_zh != steam_zh:
            stats["conflict"] += 1
            if len(conflicts) < 12:
                conflicts.append((text(cur_en)[:32], xhh_zh, steam_zh))
        if new_zh and new_zh.lower() == new_en.lower():
            stats["zh_eq_en"] += 1
        if new_zh and not CJK.search(new_zh):
            stats["junk_zh"] += 1
        if new_hant and not CJK.search(new_hant):
            stats["junk_hant"] += 1

        if (new_zh, new_en, new_hant) != (text(cur_zh), text(cur_en), text(cur_hant)):
            updates.append((new_zh, new_en, new_hant, gid))

    # ---------- 4. 孤儿检查 ----------
    game_steam = {str(r[0]) for r in conn.execute("SELECT steam_appid FROM games WHERE steam_appid IS NOT NULL")}
    game_pcgw = {str(r[0]) for r in conn.execute("SELECT pcgw_id FROM games WHERE pcgw_id IS NOT NULL")}
    orphan_steam = len({k for k in by_steam if k not in game_steam})
    orphan_xhh = len({k for k in by_pcgw if k not in game_pcgw})

    print(f"[2] 解析 {len(games):,} 个游戏，需要更新 {len(updates):,} 行")
    print(f"    name_zh      小黑盒 {stats['zh_from_xhh']:,} / Steam {stats['zh_from_steam']:,} "
          f"/ 保留 {stats['zh_kept']:,}")
    print(f"    name_en      Steam {stats['en_from_steam']:,} / 小黑盒 {stats['en_from_xhh']:,} "
          f"/ 保留 {stats['en_kept']:,}")
    print(f"    name_zh_hant Steam {stats['hant_from_steam']:,}")
    print()
    print("[3] 问题清单")
    print(f"    ① 无任何来源行的游戏        {stats['no_source']:,}")
    print(f"    ② 两源中文名冲突(都含汉字)  {stats['conflict']:,}")
    for en, a, b in conflicts:
        print(f"        {en:34s} 小黑盒「{a}」 vs Steam「{b}」")
    print(f"    ③ name_zh 与 name_en 相同    {stats['zh_eq_en']:,}")
    print(f"    ④ name_zh 不含汉字           {stats['junk_zh']:,}")
    print(f"    ⑤ name_zh_hant 不含汉字      {stats['junk_hant']:,}")
    print(f"    ⑥ 孤儿来源（不在 games 里）  Steam {orphan_steam:,} 个 appid / 小黑盒 {orphan_xhh:,} 个 pcgw_id")

    if not args.apply:
        print("\n[dry-run] 未写库。加 --apply 执行。")
        conn.close()
        return 0

    # ---------- 5. 备份 + 写入 ----------
    conn.execute("DROP TABLE IF EXISTS games_name_backup")
    conn.execute("CREATE TABLE games_name_backup AS SELECT id, name_en, name_zh, name_zh_hant FROM games")
    with conn:
        conn.executemany("UPDATE games SET name_zh = ?, name_en = ?, name_zh_hant = ? WHERE id = ?", updates)
    print(f"\n[4] 已写库，{len(updates):,} 行更新；备份表 games_name_backup 可回滚")
    conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
