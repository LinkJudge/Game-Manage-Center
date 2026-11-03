#!/usr/bin/env python3
"""导出「不标准的中文名」清单，供人工过目与日后重采。

背景：我们采的是小黑盒的**主标题**，不是「中文标题」。部分小众游戏没做汉化，
主标题原样就是日文或英文，于是被「含汉字即算中文名」的规则写了进来。

判定分四类（顺序敏感，先命中的归类）：
  日文假名      名字里含平假名/片假名 —— 基本可判非中文
  中英拼写      中文名后面跟了「 / English」「 - English」这类副标题
  英文主体      名字主体是英文，只在括号里带了中文（如 `1001 Jigsaw (拼图)`）
  中文主体英文注释  中文名后括号里放英文原名（如 `诈欺谜案（A Case of Fraud）`）
  其他含拉丁    名字夹了拉丁字母但结构不明显，需人工看

排除项：`3D迷你高尔夫`、`9-nine-九次九日九重色`、`骇客时空GU 最终编码` 这类
拉丁字母本就是名字一部分的正常情况（前缀是 2D/3D/数字，或拉丁出现在中文中间）。

用法：
  python scripts/zh-names/export_nonstandard.py                 # 写 CSV + 打印汇总
  python scripts/zh-names/export_nonstandard.py --out x.csv
"""

from __future__ import annotations

import argparse
import csv
import re
import sqlite3
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DB = ROOT / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"
DEFAULT_OUT = ROOT.parent / ".workbuddy" / "reports" / "zh-names-nonstandard.csv"

CJK = re.compile(r"[\u4e00-\u9fff]")
KANA = re.compile(r"[\u3040-\u30ff]")
LAT = re.compile(r"[A-Za-z]")
SEP = re.compile(r"\s+[/\-–—|]\s+")
PAREN = re.compile(r"[（(][^）)]*[）)]")
# 前缀是 2D/3D/4D/5D 或纯数字，后面直接跟汉字 —— 属正常中文名
LEAD = re.compile(r"^(?:[0-9]+D|[0-9]+)[\s]*[\u4e00-\u9fff]")

ADVICE = {
    "日文假名": "判非中文，不写 name_zh，另记 name_ja",
    "中英拼写": "取分隔符前的中文段",
    "英文主体": "取括号内的中文；括号里没有中文则判非中文",
    "中文主体英文注释": "去掉括号内英文，保留中文主体",
    "其他含拉丁": "人工判断；拉丁是名字一部分则保留",
}


def strip_paren(s: str) -> str:
    return PAREN.sub("", s).strip()


def classify(zh: str) -> str | None:
    """返回分类名；None 表示正常，不需要过目。"""
    if KANA.search(zh):
        return "日文假名"
    if not LAT.search(zh):
        return None
    if LEAD.match(zh):
        return None
    body = strip_paren(zh)
    has_cjk_body = bool(CJK.search(body))
    has_lat_body = bool(LAT.search(body))
    if not has_cjk_body and CJK.search(zh):
        return "英文主体"
    if has_cjk_body and not has_lat_body and LAT.search(zh):
        return "中文主体英文注释"
    if SEP.search(zh):
        return "中英拼写"
    if has_cjk_body and has_lat_body:
        return "其他含拉丁"
    return None


def latin_ratio(s: str) -> float:
    """拉丁字母占「拉丁+汉字」的比例。比例越高，越可能是英文没汉化。"""
    lat = sum(1 for ch in s if ch.isascii() and ch.isalpha())
    cjk = sum(1 for ch in s if CJK.match(ch))
    return lat / max(1, lat + cjk)


def priority(cat: str, zh: str) -> str:
    """高=基本可判不标准；中=需要看一眼；低=大概率是正常名字。"""
    if cat in ("日文假名", "英文主体"):
        return "高"
    if cat in ("中英拼写", "中文主体英文注释"):
        return "高"
    ratio = latin_ratio(zh)
    if ratio >= 0.5:
        return "高"
    if ratio >= 0.3:
        return "中"
    return "低"


def main() -> int:
    ap = argparse.ArgumentParser(description="导出不标准中文名清单")
    ap.add_argument("--out", default=str(DEFAULT_OUT), help="CSV 输出路径")
    ap.add_argument("--quiet", action="store_true", help="只写文件不打印明细")
    args = ap.parse_args()

    conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        rows = conn.execute(
            """
            SELECT g.pcgw_id, g.steam_appid, g.name_en, g.name_zh
            FROM games g
            WHERE g.name_zh IS NOT NULL AND trim(g.name_zh) <> ''
            ORDER BY g.name_en COLLATE NOCASE
            """
        ).fetchall()
        src = {
            pid: (name, status)
            for pid, name, status in conn.execute(
                "SELECT pcgw_id, name, status FROM name_sources WHERE source='xiaoheihe'"
            )
        }
    finally:
        conn.close()

    flagged = []
    for pid, appid, en, zh in rows:
        cat = classify(zh or "")
        if not cat:
            continue
        src_name, src_status = src.get(pid, (None, None))
        flagged.append(
            {
                "分类": cat,
                "优先级": priority(cat, zh or ""),
                "拉丁占比": round(latin_ratio(zh or ""), 2),
                "建议动作": ADVICE[cat],
                "pcgw_id": pid,
                "steam_appid": appid if appid is not None else "",
                "当前name_zh": zh,
                "name_en": en or "",
                "小黑盒主标题": src_name or "",
                "来源状态": src_status or "",
            }
        )

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    fields = [
        "分类",
        "优先级",
        "拉丁占比",
        "建议动作",
        "pcgw_id",
        "steam_appid",
        "当前name_zh",
        "name_en",
        "小黑盒主标题",
        "来源状态",
    ]
    order = ["日文假名", "英文主体", "中英拼写", "中文主体英文注释", "其他含拉丁"]
    prio_rank = {"高": 0, "中": 1, "低": 2}
    flagged.sort(key=lambda r: (order.index(r["分类"]), prio_rank[r["优先级"]], -r["拉丁占比"]))
    with out.open("w", encoding="utf-8-sig", newline="") as fh:
        writer = csv.DictWriter(fh, fieldnames=fields)
        writer.writeheader()
        writer.writerows(flagged)

    counts = {k: sum(1 for r in flagged if r["分类"] == k) for k in order}
    prios = {p: sum(1 for r in flagged if r["优先级"] == p) for p in ("高", "中", "低")}
    total = len(rows)
    print(f"快照时间 {datetime.now():%Y-%m-%d %H:%M:%S}")
    print(f"name_zh 总数 {total}，标记 {len(flagged)} 条（{len(flagged) / total * 100:.1f}%）")
    for k in order:
        print(f"  {k:16s} {counts[k]:5d}")
    print(f"优先级 高 {prios['高']} / 中 {prios['中']} / 低 {prios['低']}（低=大概率正常，可忽略）")
    print(f"CSV -> {out}")
    if not args.quiet:
        for k in order:
            sample = [r for r in flagged if r["分类"] == k and r["优先级"] != "低"][:3]
            if sample:
                print(f"\n--- {k} 示例 ---")
                for r in sample:
                    print(f"  [{r['pcgw_id']}] {r['name_en']!r} -> {r['当前name_zh']!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
