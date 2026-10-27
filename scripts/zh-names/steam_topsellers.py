#!/usr/bin/env python3
"""抓 Steam 商店「热销榜」的 AppID 顺序，作为热门游戏的排序依据。

为什么用热销榜而不是评测数排序：
- `sort_by=Reviews_DESC` 这个参数 Steam 并未真正支持，返回的是默认相关度（实测前 100 名
  里连 CS2 / Dota2 / PUBG 都没有）
- `filter=topsellers` 返回的 total_count ≈ 9,916，正好覆盖「玩家多、卖得好」这一档，
  顺序即 Steam 自己的热销排名，天然就是我们要的「前 1 万热门游戏」

输出：`<项目根>/.workbuddy/steam-topsellers.json`
  [{"rank": 1, "appid": "4115450"}, ...]

用法：
  python scripts/zh-names/steam_topsellers.py            # 约 100 个请求，3~5 分钟
  python scripts/zh-names/steam_topsellers.py --max-pages 5   # 只抓前 500 个调试
"""

from __future__ import annotations

import argparse
import json
import random
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT.parent / ".workbuddy" / "steam-topsellers.json"

UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
)
URL = (
    "https://store.steampowered.com/search/results/"
    "?query&start={start}&count=100&filter=topsellers&sort_by=_ASC&infinite=1"
)
APPID = re.compile(r'data-ds-appid="(\d+)"')


def fetch_page(start: int, timeout: int = 25) -> tuple[list[str], int]:
    request = urllib.request.Request(
        URL.format(start=start),
        headers={
            "User-Agent": UA,
            "Accept": "application/json, text/javascript, */*; q=0.01",
            "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8",
            "Referer": "https://store.steampowered.com/search/?filter=topsellers",
        },
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        payload = json.loads(response.read().decode("utf-8"))
    html = payload.get("results_html") or ""
    return APPID.findall(html), int(payload.get("total_count") or 0)


def main() -> int:
    parser = argparse.ArgumentParser(description="抓 Steam 热销榜 AppID 顺序")
    parser.add_argument("--max-pages", type=int, default=0, help="最多抓多少页（0=全部）")
    parser.add_argument("--start", type=int, default=0, help="从第几个开始（断点续抓）")
    parser.add_argument("--resume", action="store_true", help="合并已有输出，不覆盖")
    parser.add_argument("--min-delay", type=float, default=1.5)
    parser.add_argument("--max-delay", type=float, default=3.0)
    args = parser.parse_args()

    seen: set[str] = set()
    ranked: list[dict[str, object]] = []
    if args.resume and OUT.exists():
        try:
            ranked = json.loads(OUT.read_text(encoding="utf-8"))
            seen = {str(item["appid"]) for item in ranked}
            print(f"续抓：已有 {len(ranked)} 个 AppID")
        except Exception:  # noqa: BLE001
            ranked, seen = [], set()
    total_count = 0
    start = args.start
    page = start // 100

    while True:
        page += 1
        if args.max_pages and page > args.max_pages:
            break
        for attempt in range(3):
            try:
                ids, total_count = fetch_page(start)
                break
            except urllib.error.HTTPError as error:
                print(f"  第 {page} 页 HTTP {error.code}，重试 {attempt + 1}/3", flush=True)
                time.sleep(5 * (attempt + 1))
            except Exception as error:  # noqa: BLE001
                print(f"  第 {page} 页失败：{error}，重试 {attempt + 1}/3", flush=True)
                time.sleep(3 * (attempt + 1))
        else:
            print(f"第 {page} 页连续失败，停止。", flush=True)
            break

        if not ids:
            print(f"第 {page} 页无数据，停止。", flush=True)
            break

        added = 0
        for appid in ids:
            if appid in seen:
                continue
            seen.add(appid)
            ranked.append({"rank": len(ranked) + 1, "appid": appid})
            added += 1

        print(
            f"第 {page:3d} 页  start={start:<6d} 本页 {len(ids):3d} 新增 {added:3d} "
            f"累计 {len(ranked):5d} / 预计 {total_count}",
            flush=True,
        )

        start += 100
        if total_count and start >= total_count:
            break
        time.sleep(random.uniform(args.min_delay, args.max_delay))

    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(
        json.dumps(ranked, ensure_ascii=False, indent=1), encoding="utf-8"
    )
    print(f"\n完成：{len(ranked)} 个 AppID（Steam 报告总数 {total_count}）")
    print(f"已写入 {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
