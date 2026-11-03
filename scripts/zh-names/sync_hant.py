"""把 steam_names.name_zh_hant 全量同步到 games.name_zh_hant（方案 2）。

规则（戢翎 2026-11-02 定）：
  - 全量同步：有 Steam 记录的游戏都重写，不只补空
  - 繁体过滤：Steam 对没有繁体版的游戏会回退返回简体名，
    用 zhconv 反推——能转成"不同的繁体"说明原文是简体，一律不写入（清空）

显示层另有回退规则（前端 gameNamePair）：
  简体：name_zh → name_zh_hant → name_en
  繁体：name_zh_hant → name_en（不回退简体）
  英文：name_en

用法：python scripts/zh-names/sync_hant.py [--apply]
"""

from __future__ import annotations

import argparse
import sqlite3
import sys
import time
from pathlib import Path

from zhconv import convert

DB = Path(__file__).resolve().parents[2] / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"


def is_real_hant(text: str) -> bool:
    """能转成不同的繁体 = 原文含简体，不算真繁体。"""
    return bool(text.strip()) and convert(text, "zh-tw") == text


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true", help="真正写库（默认只统计）")
    args = ap.parse_args()

    started = time.time()
    conn = sqlite3.connect(f"file:{DB.as_posix()}?mode=rw", uri=True, timeout=30)
    conn.execute("PRAGMA busy_timeout=30000")

    raw = {
        str(appid): (name or "").strip()
        for appid, name in conn.execute(
            "SELECT steam_appid, name_zh_hant FROM steam_names"
        )
    }
    games = conn.execute(
        "SELECT id, steam_appid, name_zh_hant FROM games WHERE steam_appid IS NOT NULL"
    ).fetchall()

    keep, drop, unchanged = [], [], 0
    for gid, appid, old in games:
        value = raw.get(str(appid), "")
        if value and is_real_hant(value):
            if value != (old or ""):
                keep.append((value, gid))
        else:
            # 没采到、或其实是简体 → 清空（显示层会回退）
            if old or "":
                drop.append((gid,))
            else:
                unchanged += 1

    print(f"steam_names 记录 {len(raw):,} 个 appid；games 有 appid 的行 {len(games):,}")
    print(f"  写入真繁体：{len(keep):,} 行")
    print(f"  清空（无繁中/实为简体）：{len(drop):,} 行")
    print(f"  本来就没值：{unchanged:,} 行")

    if not args.apply:
        print("\n[dry-run] 未写库，加 --apply 执行")
        conn.close()
        return 0

    with conn:
        if keep:
            conn.executemany("UPDATE games SET name_zh_hant = ? WHERE id = ?", keep)
        if drop:
            conn.executemany("UPDATE games SET name_zh_hant = NULL WHERE id = ?", drop)

    total = conn.execute(
        "SELECT COUNT(*) FROM games WHERE name_zh_hant IS NOT NULL AND trim(name_zh_hant) <> ''"
    ).fetchone()[0]
    conn.close()
    print(f"\n完成：games.name_zh_hant 非空 {total:,} 条，用时 {time.time() - started:.1f} 秒")
    return 0


if __name__ == "__main__":
    sys.exit(main())
