#!/usr/bin/env python3
"""抓 Steam 商店各榜单的 AppID 顺序，作为「优先采哪些游戏」的排序依据。

为什么用 Steam 榜单而不是小黑盒榜单：
- Steam 搜索接口是公开的，纯 HTML 解析，零风控成本
- 小黑盒榜单是内部接口，多一种请求就多一份封禁风险（10-28 已被封过一次）

支持四个筛选：
  topsellers   热销榜（玩家多、卖得好，total ≈ 9,900）
  newreleases  新品榜（按发售日期倒序）
  comingsoon   即将发售
  specials     当前特惠

输出（每个榜单一个文件）：`<项目根>/.workbuddy/steam-rank-<filter>.json`
  [{"rank": 1, "appid": "4115450"}, ...]

`--combine` 把已抓到的所有榜单合成 `.workbuddy/steam-priority.json`，
每个 AppID 保留它在任一榜单里的最好名次，并记下来自哪些榜单。

用法：
  python scripts/zh-names/steam_topsellers.py --filter topsellers   # 约 100 请求，3~5 分钟
  python scripts/zh-names/steam_topsellers.py --filter newreleases,comingsoon,specials
  python scripts/zh-names/steam_topsellers.py --combine             # 只合并，不发请求
  python scripts/zh-names/steam_topsellers.py --filter topsellers --max-pages 5   # 调试
"""

from __future__ import annotations

import argparse
import glob
import json
import random
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STATE = ROOT.parent / ".workbuddy"

UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
)
URL = (
    "https://store.steampowered.com/search/results/"
    "?query&start={start}&count=100&filter={filter}&sort_by={sort}&infinite=1"
)
APPID = re.compile(r'data-ds-appid="(\d+)"')

# 通道选择（实测，2026-10-30）：
#   直连 Steam 商店        → 超时（WinError 10060）
#   沙箱代理 127.0.0.1:51731 → 直接失败 / 中途 502
#   Clash  127.0.0.1:7897    → 200，0.6 秒
# 所以必须显式给 --proxy；留空则直连（换到能直连的网络时可用）。
_OPENER: urllib.request.OpenerDirector = urllib.request.build_opener(
    urllib.request.ProxyHandler({})
)


def set_proxy(url: str) -> None:
    global _OPENER
    if url:
        _OPENER = urllib.request.build_opener(
            urllib.request.ProxyHandler({"http": url, "https": url})
        )
    else:
        _OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def _opener() -> urllib.request.OpenerDirector:
    return _OPENER

# 每个筛选对应的排序：热销/特惠用默认相关度（即 Steam 自己的榜序），
# 新品用发售日倒序，即将发售用发售日正序
SORTS = {
    "topsellers": "_ASC",
    "newreleases": "Released_DESC",
    "comingsoon": "Released_ASC",
    "specials": "_ASC",
}


def out_path(name: str) -> Path:
    return STATE / f"steam-rank-{name}.json"


def fetch_page(flt: str, start: int, timeout: int = 25) -> tuple[list[str], int]:
    request = urllib.request.Request(
        URL.format(start=start, filter=flt, sort=SORTS.get(flt, "_ASC")),
        headers={
            "User-Agent": UA,
            "Accept": "application/json, text/javascript, */*; q=0.01",
            "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.8",
            "Referer": f"https://store.steampowered.com/search/?filter={flt}",
        },
    )
    with _opener().open(request, timeout=timeout) as response:
        payload = json.loads(response.read().decode("utf-8"))
    html = payload.get("results_html") or ""
    return APPID.findall(html), int(payload.get("total_count") or 0)


def crawl(flt: str, max_pages: int, start_at: int, resume: bool,
          min_delay: float, max_delay: float) -> int:
    target = out_path(flt)
    seen: set[str] = set()
    ranked: list[dict[str, object]] = []
    if resume and target.exists():
        try:
            ranked = json.loads(target.read_text(encoding="utf-8"))
            seen = {str(item["appid"]) for item in ranked}
            print(f"[{flt}] 续抓：已有 {len(ranked)} 个 AppID")
        except Exception:  # noqa: BLE001
            ranked, seen = [], set()

    total_count = 0
    start = start_at
    page = start // 100

    while True:
        page += 1
        if max_pages and page > max_pages:
            break
        for attempt in range(3):
            try:
                ids, total_count = fetch_page(flt, start)
                break
            except urllib.error.HTTPError as error:
                print(f"[{flt}] 第 {page} 页 HTTP {error.code}，重试 {attempt + 1}/3", flush=True)
                time.sleep(5 * (attempt + 1))
            except Exception as error:  # noqa: BLE001
                print(f"[{flt}] 第 {page} 页失败：{error}，重试 {attempt + 1}/3", flush=True)
                time.sleep(3 * (attempt + 1))
        else:
            print(f"[{flt}] 第 {page} 页连续失败，停止。", flush=True)
            break

        if not ids:
            print(f"[{flt}] 第 {page} 页无数据，停止。", flush=True)
            break

        added = 0
        for appid in ids:
            if appid in seen:
                continue
            seen.add(appid)
            ranked.append({"rank": len(ranked) + 1, "appid": appid})
            added += 1

        print(
            f"[{flt}] 第 {page:3d} 页  start={start:<6d} 本页 {len(ids):3d} 新增 {added:3d} "
            f"累计 {len(ranked):5d} / 预计 {total_count}",
            flush=True,
        )

        start += 100
        if total_count and start >= total_count:
            break
        time.sleep(random.uniform(min_delay, max_delay))

    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(ranked, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"[{flt}] 完成：{len(ranked)} 个 AppID（Steam 报告总数 {total_count}）-> {target.name}")
    return len(ranked)


def combine() -> int:
    """把各榜单文件合成一份优先序：同一 AppID 取最好名次，并记录来自哪些榜单。"""
    best: dict[str, dict[str, object]] = {}
    files = sorted(glob.glob(str(STATE / "steam-rank-*.json")))
    # 兼容最早的 steamsellers 文件名
    legacy = STATE / "steam-topsellers.json"
    if legacy.exists():
        files.append(str(legacy))
    if not files:
        print("[合并] 没找到任何 steam-rank-*.json，先跑一次抓取。")
        return 1

    for path in files:
        name = Path(path).stem.replace("steam-rank-", "").replace("steam-topsellers", "topsellers")
        try:
            items = json.loads(Path(path).read_text(encoding="utf-8"))
        except Exception as error:  # noqa: BLE001
            print(f"[合并] 跳过 {Path(path).name}：{error}")
            continue
        print(f"[合并] {name}: {len(items)} 条")
        for item in items:
            appid = str(item["appid"])
            rank = int(item.get("rank") or 10**9)
            entry = best.get(appid)
            if entry is None:
                best[appid] = {"rank": rank, "appid": appid, "lists": [name]}
            else:
                if rank < int(entry["rank"]):
                    entry["rank"] = rank
                if name not in entry["lists"]:
                    entry["lists"].append(name)  # type: ignore[union-attr]

    ranked = sorted(best.values(), key=lambda e: (int(e["rank"]), str(e["appid"])))
    target = STATE / "steam-priority.json"
    target.write_text(json.dumps(ranked, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"[合并] 去重后 {len(ranked)} 个 AppID -> {target}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="抓 Steam 各榜单 AppID 顺序")
    parser.add_argument("--filter", default="topsellers",
                        help="topsellers/newreleases/comingsoon/specials，可逗号分隔多个")
    parser.add_argument("--combine", action="store_true", help="只合并已有榜单，不发请求")
    parser.add_argument("--max-pages", type=int, default=0, help="每个榜单最多抓多少页（0=全部）")
    parser.add_argument("--start", type=int, default=0, help="从第几个开始（断点续抓）")
    parser.add_argument("--resume", action="store_true", help="合并已有输出，不覆盖")
    parser.add_argument("--min-delay", type=float, default=1.5)
    parser.add_argument("--max-delay", type=float, default=3.0)
    parser.add_argument("--proxy", default="",
                        help="代理地址，如 http://127.0.0.1:7897；留空=直连（本机直连超时，必须给）")
    args = parser.parse_args()

    set_proxy(args.proxy)
    print(f"[通道] {'代理 ' + args.proxy if args.proxy else '直连'}", flush=True)

    if args.combine:
        return combine()

    filters = [f.strip() for f in args.filter.split(",") if f.strip()]
    unknown = [f for f in filters if f not in SORTS]
    if unknown:
        print(f"[错误] 不认识的 filter：{', '.join(unknown)}（可选 {', '.join(SORTS)}）")
        return 1

    for flt in filters:
        crawl(flt, args.max_pages, args.start, args.resume, args.min_delay, args.max_delay)
        time.sleep(random.uniform(2.0, 4.0))
    return 0


if __name__ == "__main__":
    sys.exit(main())
