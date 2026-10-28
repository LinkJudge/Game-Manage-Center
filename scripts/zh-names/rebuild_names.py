#!/usr/bin/env python3
"""按「官方 > 小黑盒 > 无」重建游戏主名与中文名 —— 默认只出报告，不落库。

⚠️ 戢翎 2026-10-28 指令：数据工程先做，但**不落库、不进软件，生效留给 0.5 版本**。
所以本脚本默认只读，写库必须显式 `--apply`，且会强制先备份。

重建规则（戢翎拍板）：
  中文名 zh_CN
    1. Steam 官方中文  game_name_official.status = 'ok'
    2. 小黑盒中文      game_name_source.status  = 'named' (source='xiaoheihe')
    3. 都没有          -> NULL（**清空上游作者人工整理的兜底中文名**）
  英文主名 title（PRIMARY KEY，只对差异项改）
    1. Steam 官方英文  game_name_official.name_en（非空）
    2. 否则           保留现有 title（覆盖无 steam_id 的条目）

用法：
  python scripts/zh-names/rebuild_names.py                      # 出报告（只读）
  python scripts/zh-names/rebuild_names.py --csv out.csv        # 导出差异明细
  python scripts/zh-names/rebuild_names.py --apply              # 0.5 才用：落库

报告会标出两类必须人工确认的事：
  - title 主键冲突（两个游戏抢同一个官方英文名）
  - title 变更会影响备份目录名（backup.rs 用 game_dir_name(title) 命名目录）
"""

from __future__ import annotations

import argparse
import csv
import shutil
import sqlite3
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DB = ROOT / "src-tauri" / "resources" / "database.db"
BACKUP_DIR = ROOT.parent / ".workbuddy" / "database-backups"


def build_plan(conn: sqlite3.Connection) -> list[dict]:
    """算出每一行游戏的新旧名字与变化类型。"""
    official = {
        str(r[0]): (r[1], r[2], r[3])
        for r in conn.execute(
            "SELECT steam_appid, name_zh, name_en, status FROM game_name_official"
        )
    }
    xhh = {}
    for page_id, name, status in conn.execute(
        "SELECT wiki_page_id, name, status FROM game_name_source WHERE source='xiaoheihe'"
    ):
        xhh[str(page_id)] = (name, status)

    rows = []
    for title, page_id, steam_id, zh_cn in conn.execute(
        "SELECT title, wiki_page_id, steam_id, zh_CN FROM games"
    ):
        off = official.get(str(steam_id)) if steam_id is not None else None
        name_zh, name_en, off_status = off if off else (None, None, None)
        xhh_name, xhh_status = xhh.get(str(page_id), (None, None))

        # 中文名：官方 > 小黑盒 > 清空
        if off_status == "ok" and name_zh:
            new_zh, zh_src = name_zh, "official"
        elif xhh_status == "named" and xhh_name:
            new_zh, zh_src = xhh_name, "xiaoheihe"
        else:
            new_zh, zh_src = None, "none"

        # 英文名：官方优先，保留原值兜底
        new_title = (name_en or "").strip() or title
        title_src = "official" if (name_en or "").strip() and new_title != title else (
            "official(same)" if (name_en or "").strip() else "keep"
        )

        old_zh = (zh_cn or "").strip() or None
        new_zh_v = (new_zh or "").strip() or None

        if old_zh == new_zh_v:
            zh_change = "same"
        elif old_zh is None:
            zh_change = "add"
        elif new_zh_v is None:
            zh_change = "clear"      # 人工兜底被清空
        else:
            zh_change = "modify"

        rows.append(
            {
                "wiki_page_id": page_id,
                "steam_id": steam_id,
                "old_title": title,
                "new_title": new_title,
                "title_change": "change" if new_title != title else "same",
                "title_src": title_src,
                "old_zh": old_zh,
                "new_zh": new_zh_v,
                "zh_change": zh_change,
                "zh_src": zh_src,
            }
        )
    return rows


def report(rows: list[dict], csv_path: Path | None) -> None:
    total = len(rows)
    t_change = sum(1 for r in rows if r["title_change"] == "change")
    zh_add = sum(1 for r in rows if r["zh_change"] == "add")
    zh_mod = sum(1 for r in rows if r["zh_change"] == "modify")
    zh_clear = sum(1 for r in rows if r["zh_change"] == "clear")
    zh_same = sum(1 for r in rows if r["zh_change"] == "same")
    zh_have_before = sum(1 for r in rows if r["old_zh"])
    zh_have_after = sum(1 for r in rows if r["new_zh"])

    print("=" * 68)
    print("游戏名重建 · 对比报告（只读，未修改数据库）")
    print("=" * 68)
    print(f"游戏总数            {total}")
    print()
    print("-- 中文名 zh_CN --")
    print(f"  改动前有中文名    {zh_have_before}  ({zh_have_before / total * 100:.1f}%)")
    print(f"  改动后有中文名    {zh_have_after}  ({zh_have_after / total * 100:.1f}%)")
    print(f"  新增              {zh_add}")
    print(f"  覆盖修改          {zh_mod}")
    print(f"  ⚠️ 清空人工兜底   {zh_clear}")
    print(f"  无变化            {zh_same}")
    print(f"  来源分布          ", end="")
    src = {}
    for r in rows:
        if r["new_zh"]:
            src[r["zh_src"]] = src.get(r["zh_src"], 0) + 1
    print(" / ".join(f"{k}={v}" for k, v in sorted(src.items())) or "无")
    print()
    print("-- 英文主名 title（PRIMARY KEY）--")
    print(f"  会变更            {t_change}  ({t_change / total * 100:.1f}%)")
    print()
    if zh_clear:
        print("⚠️ 「清空人工兜底」= 严格执行「抛弃原作者人工整理」：")
        print("   这些游戏将不再有中文名，界面回退显示英文原名。")
        print()
    if t_change:
        print("🔴 title 变更的两个连带影响（0.5 落地前必须处理）：")
        print("   1) 备份目录名 = game_dir_name(title)（backup.rs:392），")
        print("      改名后新备份进新目录，旧备份留在旧目录 → 同一游戏备份分裂。")
        print("      目录归属靠 game_index.json 的 wiki_page_id，可做目录迁移。")
        print("   2) 修改器匹配用 title 做名称归一化，改名会影响匹配结果。")
        print()

    # 主键冲突检测
    seen: dict[str, list] = {}
    for r in rows:
        seen.setdefault(r["new_title"], []).append(r["old_title"])
    conflicts = {k: v for k, v in seen.items() if len(v) > 1}
    print(f"主键冲突（同一官方英文名被多个游戏占用）: {len(conflicts)}")
    for k, v in list(conflicts.items())[:5]:
        print(f"   {k!r} <- {v}")
    if len(conflicts) > 5:
        print(f"   ... 还有 {len(conflicts) - 5} 组，见 CSV")

    if csv_path:
        csv_path.parent.mkdir(parents=True, exist_ok=True)
        with csv_path.open("w", newline="", encoding="utf-8-sig") as fh:
            writer = csv.DictWriter(
                fh,
                fieldnames=[
                    "wiki_page_id", "steam_id", "old_title", "new_title",
                    "title_change", "title_src", "old_zh", "new_zh",
                    "zh_change", "zh_src",
                ],
            )
            writer.writeheader()
            for r in rows:
                if r["title_change"] == "change" or r["zh_change"] != "same":
                    writer.writerow(r)
        print(f"\n差异明细已导出：{csv_path}")


def apply_changes(conn: sqlite3.Connection, rows: list[dict]) -> None:
    """0.5 才调用。事务内一次性重建。"""
    todo = [r for r in rows if r["title_change"] == "change" or r["zh_change"] != "same"]
    print(f"准备写入 {len(todo)} 行……")
    with conn:
        for r in todo:
            conn.execute(
                "UPDATE games SET title = ?, zh_CN = ? WHERE wiki_page_id = ?",
                (r["new_title"], r["new_zh"], r["wiki_page_id"]),
            )
    print(f"已写入 {len(todo)} 行。")


def main() -> int:
    parser = argparse.ArgumentParser(description="重建游戏名（默认只读出报告）")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--csv", type=Path, help="导出差异明细 CSV")
    parser.add_argument(
        "--apply", action="store_true", help="真正落库（0.5 才用，会先备份数据库）"
    )
    args = parser.parse_args()

    if not args.db.exists():
        print(f"[错误] 找不到数据库：{args.db}")
        return 1

    conn = sqlite3.connect(args.db)
    rows = build_plan(conn)
    report(rows, args.csv)

    if not args.apply:
        print("\n[只读模式] 未修改数据库。加 --apply 才会落库（当前阶段留给 0.5）。")
        conn.close()
        return 0

    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    backup = BACKUP_DIR / f"database-before-rebuild-{stamp}.db"
    conn.close()
    shutil.copy2(args.db, backup)
    print(f"\n已备份：{backup}")

    conn = sqlite3.connect(args.db)
    try:
        apply_changes(conn, rows)
    except Exception as error:  # noqa: BLE001
        conn.close()
        print(f"[失败] {type(error).__name__}: {error}")
        print(f"数据库未受影响，可从备份恢复：{backup}")
        return 1
    conn.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
