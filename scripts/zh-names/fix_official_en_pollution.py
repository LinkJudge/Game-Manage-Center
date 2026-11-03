"""修正 apply_official_en 的污染：把「官方英文名其实是中文」的行还原。

背景：`steam_names.name_en` 里有约 264 条是中文（早期脚本把中文同时写进 name_en 的遗留），
2026-11-01 apply_official_en.py 无条件照搬，把原本正确的 wiki 英文名覆盖成了中文。

做法：
  1. 从备份库里取回这些行的原始 name_en
  2. 把 steam_names 里含汉字的 name_en 置空（它们不是英文名，不该参与后续任何切换）
  3. 复查结果
"""

from __future__ import annotations

import re
import sqlite3
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DB = ROOT / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"
BACKUP = ROOT.parent / ".workbuddy" / "database-backups" / "database-before-official-en-20261101-173853.db"
CJK = re.compile(r"[\u4e00-\u9fff]")


def main() -> int:
    if not BACKUP.exists():
        print(f"[错误] 找不到备份：{BACKUP}")
        return 1

    old = sqlite3.connect(f"file:{BACKUP}?mode=ro", uri=True)
    before = {pid: en for pid, en in old.execute("SELECT pcgw_id, name_en FROM games")}
    old.close()

    conn = sqlite3.connect(DB, timeout=30)
    conn.execute("PRAGMA busy_timeout = 30000")
    try:
        # 1) 找出被污染的：当前 name_en 含汉字，且和备份不同
        rows = conn.execute("SELECT pcgw_id, name_en FROM games").fetchall()
        restore = []
        for pid, current in rows:
            if not CJK.search(current or ""):
                continue
            original = before.get(pid)
            if original and original != current:
                restore.append((original, pid))
        print(f"待还原的行 {len(restore):,}")
        for target, pid in restore[:5]:
            cur = next(c for p, c in rows if p == pid)
            print(f"  [{pid}] {cur!r} → {target!r}")

        conn.executemany("UPDATE games SET name_en = ? WHERE pcgw_id = ?", restore)

        # 2) 清理 steam_names 里不是英文的 name_en
        bad = [
            (appid,)
            for appid, name in conn.execute(
                "SELECT steam_appid, name_en FROM steam_names WHERE name_en IS NOT NULL AND trim(name_en) <> ''"
            )
            if CJK.search(name)
        ]
        print(f"\nsteam_names 里含汉字的 name_en：{len(bad):,}（置空，避免再被当成英文名）")
        conn.executemany("UPDATE steam_names SET name_en = NULL WHERE steam_appid = ?", bad)

        conn.commit()

        left = sum(
            1
            for (name,) in conn.execute("SELECT name_en FROM games")
            if CJK.search(name or "")
        )
        print(f"\n还原后 games.name_en 含汉字：{left:,}（应回到切换前的水平，约 617）")
    finally:
        conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
