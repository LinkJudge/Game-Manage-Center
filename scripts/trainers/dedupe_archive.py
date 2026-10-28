#!/usr/bin/env python3
"""从风灵旧档目录里剔除与官网主目录重复的条目。

背景：两个来源有 56 个游戏重叠，且官网版本明显更强
（Anno 1800 官网 13 个包 vs 旧档 1 个；ARK 官网 2020-11 vs 旧档 2019-11）。
不做处理的话，搜同一个游戏会出现两行（官网一条、旧档一条），
用户还得自己分辨该点哪个。

参考项目 Game Cheats Manager 的做法是运行时去重（先搜 archive 再搜 main，
按归一化游戏名比对，fling_main 覆盖 fling_archive）。本项目选择在**数据层**做，
理由：构建时一次搞定，引擎 search/match 保持简单，也没有运行时开销。

去重不影响「旧档独有的 408 条」——那些是官网已下架的老修改器，
是用户拿到它们的唯一途径，全部保留。

用法：
    python scripts/trainers/dedupe_archive.py            # 去重并写回
    python scripts/trainers/dedupe_archive.py --dry-run  # 只报告不写
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
import unicodedata
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TRAINERS = ROOT / "src-tauri" / "resources" / "trainers"
MAIN_JSON = TRAINERS / "fling.json"
ARCHIVE_JSON = TRAINERS / "fling_archive.json"
BACKUP_DIR = ROOT.parent / ".workbuddy" / "trainer-backups"

# 与引擎 trainers::normalize_name 保持一致的思路：
# 数字转罗马、丢弃标点与空白、只留字母/CJK、小写。
ROMAN = {
    1: "i", 2: "ii", 3: "iii", 4: "iv", 5: "v", 6: "vi", 7: "vii", 8: "viii",
    9: "ix", 10: "x", 11: "xi", 12: "xii", 13: "xiii", 14: "xiv", 15: "xv",
    16: "xvi", 17: "xvii", 18: "xviii", 19: "xix", 20: "xx",
}


def normalize(name: str) -> str:
    """归一化游戏名用于比对（宽松：宁可多剔，也不要漏掉重复）。"""
    s = unicodedata.normalize("NFKC", name or "").lower()
    s = re.sub(r"[™®©]", "", s)
    # 数字 → 罗马（与引擎一致，让 "FIFA 24" 与 "FIFA XXIV" 视为同一游戏）
    s = re.sub(r"\d+", lambda m: ROMAN.get(int(m.group()), m.group()), s)
    # 只保留字母、数字、汉字
    s = "".join(c for c in s if c.isalnum() or "\u4e00" <= c <= "\u9fff")
    return s


def main() -> int:
    parser = argparse.ArgumentParser(description="剔除旧档里与官网重复的修改器条目")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--keep-archive", action="store_true", help="反向：以旧档为准剔除官网（一般不用）")
    args = parser.parse_args()

    for p in (MAIN_JSON, ARCHIVE_JSON):
        if not p.exists():
            print(f"[错误] 找不到 {p}")
            return 1

    main_data = json.loads(MAIN_JSON.read_text(encoding="utf-8"))
    arch_data = json.loads(ARCHIVE_JSON.read_text(encoding="utf-8"))

    base, target = (main_data, arch_data) if not args.keep_archive else (arch_data, main_data)
    base_names = {normalize(t.get("gameName", "")) for t in base["trainers"]}

    kept, dropped = [], []
    for t in target["trainers"]:
        n = normalize(t.get("gameName", ""))
        if n and n in base_names:
            dropped.append(t)
        else:
            kept.append(t)

    print(f"基准（{base['source']['id']}）  : {len(base['trainers'])} 条")
    print(f"待处理（{target['source']['id']}）: {len(target['trainers'])} 条")
    print(f"  与基准重复 → 剔除 : {len(dropped)}")
    print(f"  保留              : {len(kept)}")

    if dropped:
        print("\n剔除样例（这些在基准里有更全的版本）：")
        for t in dropped[:12]:
            print(f"  {t.get('gameName','')[:48]:50} 包={len(t.get('downloads', []))}")

    if args.dry_run:
        print("\n[dry-run] 未写入")
        return 0

    if not dropped:
        print("\n没有重复，无需改动。")
        return 0

    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    out_path = ARCHIVE_JSON if not args.keep_archive else MAIN_JSON
    backup = BACKUP_DIR / f"{out_path.stem}-{stamp}.json"
    shutil.copy2(out_path, backup)
    print(f"\n已备份：{backup}")

    target["trainers"] = kept
    target["stats"] = {**target.get("stats", {}), "count": len(kept)}
    target["generatedAt"] = datetime.now().strftime("%Y-%m-%dT%H:%M:%SZ")
    out_path.write_text(
        json.dumps(target, ensure_ascii=False, separators=(",", ":")),
        encoding="utf-8",
    )
    print(f"已写入：{out_path}（{len(kept)} 条）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
