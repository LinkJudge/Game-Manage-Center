#!/usr/bin/env python3
"""Steam 官方中文名 · 并发采集器。

为什么单独写一个：
`steam_names.py` 是串行 + 1.3~2.2 秒间隔，跑完库里 4.6 万个待采要 20 小时。
Steam 的 appdetails 接口实测很宽松（10 个连发 11 秒、零 429），
瓶颈完全在间隔而不是服务端。这里用线程池 + 全局固定速率重写一遍，
4~5 请求/秒，全量约 3 小时；原脚本保持不动，随时可回退。

判定规则与原脚本一致：
  success=false          → missing（官方无此 appid）
  返回名含汉字            → ok       （official 中文名，写入 name_zh）
  返回名不含汉字          → en_only  （官方没做中文本地化，name_zh 留空）
  网络彻底失败            → error    （可用 --retry-errors 重跑）

写库只碰 steam_names 表，不动 games.name_zh —— 目录中文名由 rebuild_names.py 统一决定。

用法：
  python scripts/zh-names/steam_names_parallel.py --probe          # 探活 20 条
  python scripts/zh-names/steam_names_parallel.py --workers 4      # 全量，约 3 小时
  python scripts/zh-names/steam_names_parallel.py --limit 500 --workers 6
"""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DB = ROOT / "src-tauri" / "resources" / "database.db"
API = "https://store.steampowered.com/api/appdetails?appids={appid}&l=schinese&cc=cn&filters=basic"
UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
)
CJK = re.compile(r"[\u3400-\u9fff]")

SCHEMA = """
CREATE TABLE IF NOT EXISTS steam_names (
    steam_appid TEXT PRIMARY KEY,
    name_zh     TEXT,
    name_en     TEXT,
    status      TEXT NOT NULL,
    checked_at  TEXT NOT NULL
)
"""

# 全局速率闸门：所有线程排队取号，保证整体不超过 1/GAP 请求/秒
_gate = threading.Lock()
_next_slot = 0.0
GAP = 0.22
_stop = threading.Event()
_stats = {"ok": 0, "en_only": 0, "missing": 0, "error": 0}
_stats_lock = threading.Lock()

# 通道：环境里的 HTTPS_PROXY=127.0.0.1:51731 对 store.steampowered.com 不通，
# 会让每条请求挂满超时。默认直连，本机要代理必须显式 --proxy http://127.0.0.1:7897。
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


def _throttle() -> None:
    global _next_slot
    with _gate:
        now = time.monotonic()
        slot = max(now, _next_slot)
        _next_slot = slot + GAP
    delay = slot - time.monotonic()
    if delay > 0:
        time.sleep(delay)


def fetch(appid: str, retries: int = 2, timeout: int = 12):
    """返回 (status, name_zh, name_en)。status ∈ ok/en_only/missing/error。"""
    last_error = None
    for attempt in range(retries + 1):
        if _stop.is_set():
            return "error", None, None
        _throttle()
        request = urllib.request.Request(API.format(appid=appid), headers={"User-Agent": UA})
        try:
            with _OPENER.open(request, timeout=timeout) as response:
                payload = json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as error:
            if error.code == 429:
                # 全局限速翻倍，休息一会儿再继续（不是单条重试）
                global GAP
                GAP = min(1.2, GAP * 2)
                print(f"      ! 429 限流，全局间隔调到 {GAP:.2f}s，暂停 45s", flush=True)
                time.sleep(45)
                last_error = error
                continue
            if error.code >= 500:
                time.sleep(2 * (attempt + 1))
                last_error = error
                continue
            return "error", None, None
        except Exception as error:  # noqa: BLE001 - 网络抖动一律按可重试处理
            time.sleep(1.5 * (attempt + 1))
            last_error = error
            continue

        entry = payload.get(appid) or {}
        if not entry.get("success"):
            return "missing", None, None
        name = str((entry.get("data") or {}).get("name") or "").strip()
        if not name:
            return "missing", None, None
        if CJK.search(name):
            return "ok", name, name
        return "en_only", None, name

    if last_error is not None:
        return "error", None, None
    return "error", None, None


def main() -> int:
    global GAP
    parser = argparse.ArgumentParser(description="Steam 官方中文名并发采集")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--workers", type=int, default=1,
                        help="并发线程数（默认 1；Steam 实测约 1 条/秒，再快就 429）")
    parser.add_argument("--gap", type=float, default=0.9, help="全局最小请求间隔秒（默认 0.9）")
    parser.add_argument("--limit", type=int, default=0, help="本轮最多处理多少（0=全部）")
    parser.add_argument("--probe", action="store_true", help="探活：20 条、2 线程")
    parser.add_argument("--retry-errors", action="store_true", help="把上轮 error 的重新排队")
    parser.add_argument("--proxy", default="", help="代理地址，如 http://127.0.0.1:7897；留空=直连")
    parser.add_argument("--hot", type=Path, default=None,
                        help="优先榜单（steam-priority.json）：只采榜上有的游戏，按名次排序")
    parser.add_argument("--hot-top", type=int, default=0, help="只取榜单前 N 名（0=榜上全部）")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    if args.probe:
        args.workers, args.limit = 2, 20
    set_proxy(args.proxy)
    GAP = max(0.05, args.gap)
    print(f"[通道] {'代理 ' + args.proxy if args.proxy else '直连'}", flush=True)

    conn = sqlite3.connect(args.db, timeout=15)
    # 与小黑盒采集并行时共用同一库，写锁要肯等
    conn.execute("PRAGMA busy_timeout = 15000")
    conn.execute(SCHEMA)
    conn.commit()

    done = {str(r[0]) for r in conn.execute("SELECT steam_appid FROM steam_names")}
    if args.retry_errors:
        errs = {str(r[0]) for r in conn.execute("SELECT steam_appid FROM steam_names WHERE status='error'")}
        done -= errs
        print(f"重跑模式：{len(errs)} 个 error 条目重新排队")

    rows = conn.execute(
        "SELECT steam_appid FROM games WHERE steam_appid IS NOT NULL "
        "AND trim(CAST(steam_appid AS TEXT)) <> ''"
    ).fetchall()
    targets: list[str] = []
    seen: set[str] = set()
    for (raw,) in rows:
        digits = "".join(ch for ch in str(raw) if ch.isdigit())
        if not digits or digits in seen or digits in done:
            continue
        seen.add(digits)
        targets.append(digits)

    # 榜单优先：Steam 实测约 1 条/秒（再快就 429），4.6 万条要 12 小时。
    # 榜上游戏才是玩家会搜的，先采这批；榜外的冷门留到以后慢慢跑。
    if args.hot:
        if not args.hot.exists():
            print(f"[错误] 找不到榜单文件：{args.hot}")
            return 1
        hot_list = json.loads(args.hot.read_text(encoding="utf-8"))
        rank = {str(item["appid"]): int(item.get("rank") or 10**9) for item in hot_list}
        before = len(targets)
        targets = [a for a in targets if a in rank]
        targets.sort(key=lambda a: rank[a])
        if args.hot_top > 0:
            targets = targets[: args.hot_top]
        print(f"榜单过滤：榜上 {len(rank)} 个 AppID，命中待采 {len(targets)} 条"
              f"（原待采 {before} 条），按名次处理", flush=True)

    total_pending = len(targets)
    if args.limit > 0:
        targets = targets[: args.limit]
    print(f"库内 AppID {len(seen)} 个，已采 {len(done)} 个，待采 {total_pending} 个；"
          f"本轮 {len(targets)} 个，{args.workers} 线程，间隔 {GAP:.2f}s", flush=True)
    if not targets:
        print("没有待处理记录。")
        return 0
    if args.dry_run:
        print("[dry-run] 前 10:", targets[:10])
        return 0

    buf: list[tuple] = []
    started = time.time()
    processed = 0

    def flush() -> None:
        if not buf:
            return
        conn.executemany(
            """
            INSERT INTO steam_names(steam_appid, name_zh, name_en, status, checked_at)
            VALUES(?, ?, ?, ?, ?)
            ON CONFLICT(steam_appid) DO UPDATE SET
                name_zh = excluded.name_zh,
                name_en = excluded.name_en,
                status = excluded.status,
                checked_at = excluded.checked_at
            """,
            buf,
        )
        conn.commit()
        buf.clear()

    try:
        with ThreadPoolExecutor(max_workers=args.workers) as pool:
            futures = {pool.submit(fetch, appid): appid for appid in targets}
            for future in as_completed(futures):
                appid = futures[future]
                try:
                    status, name_zh, name_en = future.result()
                except Exception:  # noqa: BLE001
                    status, name_zh, name_en = "error", None, None
                processed += 1
                with _stats_lock:
                    _stats[status] = _stats.get(status, 0) + 1
                buf.append((appid, name_zh, name_en, status, datetime.now().isoformat(timespec="seconds")))
                if len(buf) >= 25:
                    flush()
                if processed % 200 == 0 or processed == len(targets):
                    speed = processed / max(1e-6, time.time() - started)
                    left = (len(targets) - processed) / max(1e-6, speed)
                    print(f"  {processed}/{len(targets)}  {speed:.1f}/s  "
                          f"ok={_stats['ok']} en_only={_stats['en_only']} "
                          f"missing={_stats['missing']} error={_stats['error']}  "
                          f"预计剩余 {left/60:.0f} 分钟", flush=True)
    except KeyboardInterrupt:
        print("收到中断，落盘已完成的记录……", flush=True)
        _stop.set()

    flush()
    conn.close()
    elapsed = (time.time() - started) / 60
    print(f"完成：{processed} 条，用时 {elapsed:.1f} 分钟；"
          f"ok={_stats['ok']} en_only={_stats['en_only']} "
          f"missing={_stats['missing']} error={_stats['error']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
