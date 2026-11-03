"""把 games.name_en 全部换成 Steam 官方英文名。

戢翎 2026-11-01 决定：**英文名一律用官方**，wiki 的原始标题退为兜底（官方没有时保留）。

规则：
  - 只要该游戏的 steam_appid 在 steam_names 里有非空 name_en，就用官方的，逐字照搬
    （包括大小写、™ 等符号，不做"美化"，避免引入主观判断）
  - 官方没有的，保留现有值不动

用法：
  python scripts/zh-names/apply_official_en.py --dry-run   # 只看会改多少
  python scripts/zh-names/apply_official_en.py             # 真写库（自动备份）
"""

from __future__ import annotations

import argparse
import shutil
import sqlite3
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DB = ROOT / "src-tauri" / "resources" / "database.db"
BACKUP_DIR = ROOT.parent / ".workbuddy" / "database-backups"


def backup() -> Path:
    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    target = BACKUP_DIR / f"database-before-official-en-{datetime.now():%Y%m%d-%H%M%S}.db"
    src = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    dst = sqlite3.connect(target)
    try:
        src.backup(dst)
    finally:
        dst.close()
        src.close()
    return target


def main() -> int:
    ap = argparse.ArgumentParser(description="把 name_en 换成 Steam 官方英文名")
    ap.add_argument("--dry-run", action="store_true", help="只统计不写库")
    args = ap.parse_args()

    official = {}
    conn = sqlite3.connect(DB, timeout=30)
    conn.execute("PRAGMA busy_timeout = 30000")
    try:
        for appid, name in conn.execute(
            "SELECT steam_appid, name_en FROM steam_names "
            "WHERE name_en IS NOT NULL AND trim(name_en) <> ''"
        ):
            official[str(appid)] = name

        rows = conn.execute("SELECT pcgw_id, steam_appid, name_en FROM games").fetchall()
        updates = []
        unchanged = 0
        no_official = 0
        for pcgw_id, appid, current in rows:
            key = str(appid) if appid is not None else ""
            if key not in official:
                no_official += 1
                continue
            target = official[key]
            if (current or "") == target:
                unchanged += 1
                continue
            updates.append((target, pcgw_id))

        print(f"games 总数            {len(rows):,}")
        print(f"  官方没有英文名       {no_official:,}（保留原值）")
        print(f"  已经一致             {unchanged:,}")
        print(f"  需要改               {len(updates):,}")

        if args.dry_run:
            print("\n[dry-run] 不写库。抽样前 5 条：")
            for target, pcgw_id in updates[:5]:
                old = next(r[2] for r in rows if r[0] == pcgw_id)
                print(f"  [{pcgw_id}] {old!r} → {target!r}")
            return 0

        if not updates:
            print("没有需要改的行")
            return 0

        path = backup()
        print(f"\n已备份数据库：{path}")
        conn.executemany("UPDATE games SET name_en = ? WHERE pcgw_id = ?", updates)
        conn.commit()
        print(f"已更新 {len(updates):,} 行 name_en")
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
