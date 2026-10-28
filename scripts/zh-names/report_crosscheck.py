#!/usr/bin/env python3
"""比对「主名」（games.name_zh）与「小黑盒名」（name_sources），产出交叉验证报告。

主名来源是 PCGamingWiki 等上游数据，小黑盒是独立来源；两边不一致时不能自动覆盖，
先列出来人工判断（有些是官方译名差异，有些是民间叫法，有些是小黑盒数据不准）。

用法：
  python scripts/zh-names/report_crosscheck.py                    # 全库摘要
  python scripts/zh-names/report_crosscheck.py --hot <topsellers.json>   # 只看热销榜
  python scripts/zh-names/report_crosscheck.py --csv out.csv      # 导出差异清单
  python scripts/zh-names/report_crosscheck.py --show 30          # 控制台列出前 N 条差异
"""

from __future__ import annotations

import argparse
import csv
import json
import re
import sqlite3
import unicodedata
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DB = ROOT / "src-tauri" / "resources" / "database.db"
DEFAULT_OUT = ROOT.parent / ".workbuddy" / "zh-name-conflicts.csv"
DIGITS = re.compile(r"^\D*(\d+)")
CJK = re.compile(r"[\u3400-\u9fff]")
PUNCT = re.compile(r"[\s\u3000\-_/\\:：·,，.。!！?？'\"“”‘’()（）\[\]【】《》〈〉+&、|~]+")


def norm(text: str) -> str:
    """比较用归一化：NFKC、转小写、去空白与标点。"""
    if not text:
        return ""
    value = unicodedata.normalize("NFKC", text).lower()
    return PUNCT.sub("", value)


def steam_appid(value: object) -> str | None:
    match = DIGITS.match(str(value or ""))
    return match.group(1) if match else None


def main() -> int:
    parser = argparse.ArgumentParser(description="主名 vs 小黑盒名 交叉验证")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--hot", type=Path, default=None, help="只看这个热销榜 JSON 里的游戏")
    parser.add_argument("--csv", type=Path, default=None, help="导出差异清单（默认不导出）")
    parser.add_argument("--show", type=int, default=10, help="控制台最多列多少条差异（0=不列）")
    args = parser.parse_args()

    if not args.db.exists():
        print(f"[错误] 找不到数据库：{args.db}")
        return 1

    rank: dict[str, int] = {}
    if args.hot:
        if not args.hot.exists():
            print(f"[错误] 找不到热销榜：{args.hot}")
            return 1
        for item in json.loads(args.hot.read_text(encoding="utf-8")):
            rank[str(item["appid"])] = int(item["rank"])

    conn = sqlite3.connect(f"file:{args.db}?mode=ro", uri=True)
    conn.row_factory = sqlite3.Row
    rows = conn.execute(
        """
        SELECT g.pcgw_id AS wiki_page_id, g.name_en AS title, g.steam_appid AS steam_id, g.name_zh AS zh_CN,
               n.name AS src_name, n.status AS src_status, n.checked_at
        FROM games g
        LEFT JOIN name_sources n
          ON n.pcgw_id = g.pcgw_id AND n.source = 'xiaoheihe'
        WHERE n.pcgw_id IS NOT NULL
        """
    ).fetchall()
    conn.close()

    same: list[sqlite3.Row] = []
    conflict: list[sqlite3.Row] = []
    main_only: list[sqlite3.Row] = []
    src_only: list[sqlite3.Row] = []
    neither: list[sqlite3.Row] = []
    latin: list[sqlite3.Row] = []

    for row in rows:
        appid = steam_appid(row["steam_id"])
        if rank and (appid is None or appid not in rank):
            continue
        main = (row["zh_CN"] or "").strip()
        src = (row["src_name"] or "").strip()
        if row["src_status"] == "latin":
            # 小黑盒显示名不含中文（CS2 这类），不参与译名比对
            latin.append(row)
            continue
        if main and src:
            (same if norm(main) == norm(src) else conflict).append(row)
        elif main and not src:
            main_only.append(row)
        elif src and not main:
            src_only.append(row)
        else:
            neither.append(row)

    scope = f"热销榜范围（{len(rank)} 个 AppID）" if rank else "全库"
    total = len(same) + len(conflict) + len(main_only) + len(src_only) + len(neither) + len(latin)
    print(f"=== 交叉验证：{scope} ===")
    print(f"已有小黑盒记录的游戏：{total}")
    print(f"  两边都有名 且一致   ：{len(same):6d}")
    print(f"  两边都有名 但不一致 ：{len(conflict):6d}   ← 需要人工判断")
    print(f"  只有主名（小黑盒无） ：{len(main_only):6d}")
    print(f"  只有小黑盒名（主名空）：{len(src_only):6d}")
    print(f"  两边都没有           ：{len(neither):6d}")
    print(f"  小黑盒为非中文名     ：{len(latin):6d}   ← 如 CS2，仅记录不比对")
    if same or conflict:
        rate = len(same) / (len(same) + len(conflict)) * 100
        print(f"\n一致率：{rate:.1f}%（{len(same)}/{len(same) + len(conflict)}）")

    if args.show and conflict:
        def order(row: sqlite3.Row) -> tuple[int, str]:
            appid = steam_appid(row["steam_id"])
            return (rank.get(appid, 999999) if rank else 0, row["title"])

        print(f"\n=== 不一致示例（前 {min(args.show, len(conflict))} 条）===")
        for row in sorted(conflict, key=order)[: args.show]:
            appid = steam_appid(row["steam_id"])
            tag = f"#{rank[appid]}" if rank and appid in rank else ""
            print(f"  {tag:>7} {row['title']}")
            print(f"          主名    ：{row['zh_CN']}")
            print(f"          小黑盒名：{row['src_name']}")

    if args.csv:
        out = args.csv
        out.parent.mkdir(parents=True, exist_ok=True)
        with out.open("w", encoding="utf-8-sig", newline="") as handle:
            writer = csv.writer(handle)
            writer.writerow(["热销排名", "wiki_page_id", "steam_id", "英文名", "主名", "小黑盒名", "核对时间"])
            for row in sorted(conflict, key=lambda r: (rank.get(steam_appid(r["steam_id"]), 999999))):
                appid = steam_appid(row["steam_id"])
                writer.writerow([
                    rank.get(appid, "") if rank else "",
                    row["wiki_page_id"],
                    row["steam_id"],
                    row["title"],
                    row["zh_CN"],
                    row["src_name"],
                    row["checked_at"],
                ])
        print(f"\n差异清单已导出：{out}（{len(conflict)} 条）")

    return 0


if __name__ == "__main__":
    sys.exit(main())
