#!/usr/bin/env python3
"""把 PCGamingWiki 的新数据合并进我们的库，**只更新 PCGW 字段，绝不碰中文名**。

为什么需要它（原来的增量流程有两个坑）：

1. `sync-pcgamingwiki.js` 生成的是一个只有 `games` 表的新库，
   直接替换 `database.db` 会把 `name_sources` / `steam_names` / `extra_names`
   三张表一起抹掉 —— 所有采集进度归零，等于从头再采一遍。
2. 那个构建器会 `zh_CN: (old && old.zh_CN) || null`，从旧库沿用它记的中文名。
   我们现在的 `name_zh` 是小黑盒 + Steam 官方现采的，
   一旦被这几千条旧人工值覆盖，刚采的全白干。

所以增量拆成两步：
  A. 抓数据 → `incremental-sync.cjs` + `sync-pcgamingwiki.js`（产出 database.new.db）
  B. 合并   → 本脚本：按 pcgw_id 原地更新，中文名与三张辅助表原样保留

映射（上游列 → 我们的列）：
  title → name_en | install_folder → install_dir | steam_id → steam_appid
  gog_id → gog_appid | save_location → save_paths | platform → stores
  zh_CN → **丢弃**

用法：
  python scripts/db/apply_pcgw_delta.py --dry-run    # 先看会改什么
  python scripts/db/apply_pcgw_delta.py              # 真合并（自动备份）
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sqlite3
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"
SOURCE = ROOT / "scripts" / "pcgw" / "database" / "database.new.db"
BACKUP_DIR = ROOT.parent / ".workbuddy" / "database-backups"

# (我们的列, 上游列)
FIELDS = [
    ("name_en", "title"),
    ("install_dir", "install_folder"),
    ("steam_appid", "steam_id"),
    ("gog_appid", "gog_id"),
    ("save_paths", "save_location"),
    ("stores", "platform"),
]

# 默认只同步"低风险"字段。
#
# install_dir / stores / save_paths 默认不动：实测上游 install_folder 与我们的
# install_dir 有 5,235 条不同，是两套推断规则打架（上游 `123kickit`、
# 我们 `1... 2... 3... KICK IT!`），没有一边绝对正确。当前值经过验证，
# 一次性覆盖 5,235 条只会引入 5,235 个未知风险 —— 要改必须显式加开关。
OPT_IN_FIELDS = {"name_en", "install_dir", "stores", "save_paths"}


def leading_int(value: object) -> int | None:
    """上游的 steam_id/gog_id 可能带注释（`2093619782 <!-- ... -->`），取开头数字。"""
    if value is None:
        return None
    match = re.match(r"\s*(\d+)", str(value))
    return int(match.group(1)) if match else None


def normalize(col: str, value: object) -> object:
    if col in ("steam_appid", "gog_appid"):
        return leading_int(value)
    if value is None:
        return None
    return str(value)


def norm_json(text: object) -> str:
    """JSON 列比较时忽略键序与空白，避免把没变的当成变了。"""
    if text is None:
        return ""
    try:
        return json.dumps(json.loads(str(text)), sort_keys=True, ensure_ascii=False)
    except Exception:  # noqa: BLE001
        return str(text)


def same(col: str, a: object, b: object) -> bool:
    if col in ("save_paths", "stores"):
        return norm_json(a) == norm_json(b)
    if col in ("steam_appid", "gog_appid"):
        return leading_int(a) == leading_int(b)
    return ("" if a is None else str(a)) == ("" if b is None else str(b))


def is_empty(col: str, value: object) -> bool:
    """新值是否等于「没有」——这种值绝不能覆盖已有的有效值。"""
    if value is None:
        return True
    if col in ("steam_appid", "gog_appid"):
        return leading_int(value) is None
    if col in ("save_paths", "stores"):
        return norm_json(value) in ("", "{}", "[]")
    return str(value).strip() == ""


def main() -> int:
    parser = argparse.ArgumentParser(description="合并 PCGamingWiki 增量数据（保护中文名）")
    parser.add_argument("--source", type=Path, default=SOURCE)
    parser.add_argument("--target", type=Path, default=TARGET)
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--update-title", action="store_true",
                        help="用 PCGW 页面标题覆盖 name_en（默认不覆盖：英文名以 Steam 官方为准，"
                             "由 rebuild_names.py 统一定夺；只有新增行会写入）")
    parser.add_argument("--update-install-dir", action="store_true",
                        help="用上游 install_folder 覆盖我们推断的 install_dir（默认不覆盖）")
    parser.add_argument("--update-stores", action="store_true",
                        help="用上游 platform 覆盖 stores（默认不覆盖）")
    parser.add_argument("--update-save-paths", action="store_true",
                        help="用上游 save_location 覆盖 save_paths（默认不覆盖）")
    args = parser.parse_args()

    if not args.source.exists():
        print(f"[错误] 源库不存在：{args.source}")
        print("       先跑 node scripts/incremental-sync.cjs && node scripts/sync-pcgamingwiki.js")
        return 1
    if not args.target.exists():
        print(f"[错误] 目标库不存在：{args.target}")
        return 1

    print(f"源库  : {args.source}")
    print(f"目标库: {args.target}")

    source = sqlite3.connect(f"file:{args.source}?mode=ro", uri=True)
    source.row_factory = sqlite3.Row
    rows = source.execute(
        "SELECT title, wiki_page_id, install_folder, steam_id, gog_id, save_location, platform FROM games"
    ).fetchall()
    print(f"源库 {len(rows)} 条")

    target = sqlite3.connect(args.target, timeout=30)
    target.row_factory = sqlite3.Row
    target.execute("PRAGMA busy_timeout = 30000")

    existing = {
        r["pcgw_id"]: dict(r)
        for r in target.execute(
            "SELECT pcgw_id, name_en, name_zh, install_dir, steam_appid, gog_appid, save_paths, stores FROM games"
        )
    }
    print(f"目标库 {len(existing)} 条")

    # 哪些高风险字段这次允许被覆盖
    opt_in = {
        "name_en": args.update_title,
        "install_dir": args.update_install_dir,
        "stores": args.update_stores,
        "save_paths": args.update_save_paths,
    }
    print("同步字段：steam_appid / gog_appid"
          + "".join(f" / {k}" for k, v in opt_in.items() if v)
          + ("（其余保持现值）" if not all(opt_in.values()) else ""))

    def enabled(col: str) -> bool:
        return opt_in.get(col, True)

    to_update: list[tuple] = []
    to_insert: list[tuple] = []
    field_changes = {col: 0 for col, _ in FIELDS}
    skipped_zh = 0
    unchanged = 0
    kept_old = 0

    for row in rows:
        pcgw_id = row["wiki_page_id"]
        values = {}
        for col, src_col in FIELDS:
            if col in OPT_IN_FIELDS and not enabled(col):
                values[col] = existing.get(row["wiki_page_id"], {}).get(col)  # 保持现值
                continue
            values[col] = normalize(col, row[src_col])
        if row["wiki_page_id"] not in existing:
            values = {col: normalize(col, row[src_col]) for col, src_col in FIELDS}
        current = existing.get(pcgw_id)
        if current is None:
            to_insert.append((
                pcgw_id, values["name_en"], values["install_dir"], values["steam_appid"],
                values["gog_appid"], values["save_paths"] or "{}", values["stores"] or "[]",
            ))
            continue
        # 空值不许覆盖已有的有效值：上游大量条目 install_folder/platform 为空，
        # 直接写会把我们已经推断出来的安装目录、平台列表清成 NULL，越同步越差。
        for col, _ in FIELDS:
            if is_empty(col, values[col]) and not is_empty(col, current.get(col)):
                values[col] = current.get(col)
                kept_old += 1
        changed = [col for col, _ in FIELDS if not same(col, current.get(col), values[col])]
        if not changed:
            unchanged += 1
            continue
        for col in changed:
            field_changes[col] += 1
        if current.get("name_en") and not values["name_en"]:
            values["name_en"] = current["name_en"]  # 上游空标题不要覆盖已有的
        to_update.append((
            values["name_en"], values["install_dir"], values["steam_appid"],
            values["gog_appid"], values["save_paths"], values["stores"], pcgw_id,
        ))
        if (current.get("name_zh") or "").strip():
            skipped_zh += 1

    print("\n=== 变更预估 ===")
    print(f"  新增游戏      : {len(to_insert)}")
    print(f"  需更新游戏    : {len(to_update)}")
    print(f"  完全无变化    : {unchanged}")
    for col, count in field_changes.items():
        print(f"    {col:14s} 变化 {count}")
    print(f"  空值被拦下、保留旧值：{kept_old} 处")
    print(f"  本次更新涉及 {skipped_zh} 条已中文名的记录（中文名保持不动）")

    if args.dry_run:
        print("\n[dry-run] 未写入。去掉 --dry-run 才会合并。")
        return 0

    if not to_update and not to_insert:
        print("\n没有需要合并的变更。")
        return 0

    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    backup = BACKUP_DIR / f"database-pre-pcgw-{stamp}.db"
    shutil.copy2(args.target, backup)
    print(f"\n已备份：{backup}")

    with target:
        if to_update:
            # 注意：SET 里没有 name_zh，一个字都不碰
            target.executemany(
                """
                UPDATE games SET
                    name_en     = ?,
                    install_dir = ?,
                    steam_appid = ?,
                    gog_appid   = ?,
                    save_paths  = ?,
                    stores      = ?
                WHERE pcgw_id = ?
                """,
                to_update,
            )
        if to_insert:
            target.executemany(
                """
                INSERT INTO games(pcgw_id, name_en, name_zh, install_dir, steam_appid, gog_appid, save_paths, stores)
                VALUES(?, ?, NULL, ?, ?, ?, ?, ?)
                """,
                to_insert,
            )

    total = target.execute("SELECT COUNT(*) FROM games").fetchone()[0]
    named = target.execute(
        "SELECT COUNT(*) FROM games WHERE name_zh IS NOT NULL AND trim(name_zh) <> ''"
    ).fetchone()[0]
    aux = {t: target.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0]
           for t in ("name_sources", "steam_names", "extra_names") if _table_exists(target, t)}
    target.close()

    print("\n=== 合并完成 ===")
    print(f"  更新 {len(to_update)} 条，新增 {len(to_insert)} 条")
    print(f"  现在共 {total} 条，其中中文名 {named} 条（应与合并前一致）")
    print(f"  辅助表未动：{aux}")
    return 0


def _table_exists(conn: sqlite3.Connection, name: str) -> bool:
    return conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (name,)
    ).fetchone() is not None


if __name__ == "__main__":
    sys.exit(main())
