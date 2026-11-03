#!/usr/bin/env python3
"""补采 Steam 官方缺失的另一种语言名称（宁可多采，不要漏采）。

背景：`steam_names_parallel.py` 每个 appid 只发一次 `l=schinese`，
有中文的游戏就只留下中文名，英文名留空 —— 结果 3,190 条 ok 行没有官方英文名。
另外只认简体：只做了繁中的游戏会被误判成"无中文"（en_only）。

补采两种，按需跑：
  english    对 status='ok' 且缺 name_en 的行，请求 l=english
  tchinese   对 status='en_only' 的行，请求 l=tchinese（补回繁中名）

安全规则（防污染，2026-10-30/11-01 两次事故的教训）：
  - 结果含汉字 → 写 name_zh；含拉丁 → 写 name_en；两者都含 → 按 lang 决定主字段，另一个不写
  - english 通道拿到含汉字的结果一律丢弃（Steam 某些游戏的"英文名"本身就是中文）
  - 只更新 name_en / name_zh 中该通道负责的那一列，绝不覆盖已有值

用法：
  python scripts/zh-names/fetch_missing_names.py --lang english --dry-run
  python scripts/zh-names/fetch_missing_names.py --lang english --proxy http://127.0.0.1:7897 --workers 2 --gap 0.8
  python scripts/zh-names/fetch_missing_names.py --lang tchinese --proxy ... --workers 2 --gap 0.8
"""

from __future__ import annotations

import argparse
import gzip
import json
import random
import re
import subprocess
import sys
import sqlite3
import sys
import threading
import time
from datetime import datetime
from pathlib import Path

import requests

# 控制台编码兜底：计划任务/无 PYTHONUTF8 环境下 stdout 默认是 GBK，
# 碰到游戏名里的 ™ ® 会直接 UnicodeEncodeError 崩掉（2026-11-02 实测）。
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except Exception:  # noqa: BLE001
        pass

ROOT = Path(__file__).resolve().parents[2]
DB = ROOT / "src-tauri" / "resources" / "database.db"
API = "https://store.steampowered.com/api/appdetails"
UA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0 Safari/537.36"
CJK = re.compile(r"[\u4e00-\u9fff]")
KANA = re.compile(r"[\u3040-\u30ff]")
LAT = re.compile(r"[A-Za-z]")

_gate = threading.Lock()
_next_slot = 0.0
BASE_GAP = 0.25  # 基础间隔（429 时自动放大，成功后缓慢收回）
GAP = BASE_GAP
_proxies: dict[str, str] | None = None
_local = threading.local()
_stats = {"written": 0, "no_name": 0, "junk": 0, "error": 0, "raw": 0}
_stats_lock = threading.Lock()
_consecutive_429 = [0]  # 连续 429 计数，够 8 次就换节点
_rotate_lock = threading.Lock()
_last_rotate = [0.0]


def rotate_node() -> None:
    """换到测速最快节点；60 秒内只换一次，避免所有线程同时打爆控制接口。"""
    with _rotate_lock:
        if time.time() - _last_rotate[0] < 60:
            return
        _last_rotate[0] = time.time()
    script = Path(__file__).resolve().parent / "clash_ctl.py"
    try:
        subprocess.run(
            [sys.executable, str(script), "pick",
             "--group", "🚀 节点选择",
             "--url", "https://store.steampowered.com/api/appdetails?appids=730&l=english&cc=us",
             "--timeout", "3000"],
            capture_output=True, timeout=180, check=False,
        )
    except Exception:  # noqa: BLE001
        pass


def session() -> requests.Session:
    s = getattr(_local, "s", None)
    if s is None:
        s = requests.Session()
        s.trust_env = False
        s.headers.update({"User-Agent": UA, "Accept": "*/*", "Connection": "keep-alive"})
        if _proxies:
            s.proxies.update(_proxies)
        _local.s = s
    return s


def throttle() -> None:
    global _next_slot
    with _gate:
        now = time.monotonic()
        slot = max(now, _next_slot)
        _next_slot = slot + GAP
    delay = slot - time.monotonic()
    if delay > 0:
        time.sleep(delay)


def fetch(appid: str, lang: str, retries: int = 2, timeout: int = 20) -> dict | None:
    """返回完整的 data 对象（不再用 filters=basic 砍字段）。

    为什么要全量：只存名字等于把「当时接口返回了什么」永久丢掉，
    以后想要发售日期/开发商/类型标签就得重新采一遍。
    """
    for attempt in range(retries + 1):
        throttle()
        try:
            r = session().get(
                API,
                params={"appids": appid, "l": lang, "cc": "us"},
                timeout=timeout,
            )
            if r.status_code == 429:
                # 自适应：放大间隔；连续多次 429 就换节点（这个 IP 已被限流，睡再久也没用）
                with _gate:
                    globals()["GAP"] = min(4.0, GAP * 1.5)
                    _consecutive_429[0] += 1
                    should_rotate = _consecutive_429[0] >= 8
                    if should_rotate:
                        _consecutive_429[0] = 0
                if should_rotate:
                    rotate_node()
                time.sleep(min(12, 2 + 2 * attempt) + random.random() * 2)
                continue
            if r.status_code >= 500:
                time.sleep(2 * (attempt + 1))
                continue
            if r.status_code != 200:
                return None
            if r.status_code == 200:
                # 成功时缓慢收回间隔（降到基础值的 95%），限流解除后自动提速
                with _gate:
                    globals()["GAP"] = max(BASE_GAP, GAP * 0.97)
                    _consecutive_429[0] = 0
            entry = (r.json() or {}).get(appid) or {}
            if not entry.get("success"):
                # 已下架/无效 appid：永远拿不到名字，必须标记成"查过、无"，
                # 否则每轮都把它当待采，白跑一遍（实测 1,000 条这样的会反复重试）
                if args.lang == "tchinese":
                    return "", None
                return None
            data = entry.get("data")
            return data if isinstance(data, dict) else None
        except Exception:  # noqa: BLE001
            time.sleep(1.5 * (attempt + 1))
    return None


def main() -> int:
    global GAP, _proxies
    ap = argparse.ArgumentParser(description="补齐 Steam 官方另一种语言的名字")
    ap.add_argument("--lang", choices=["english", "tchinese", "japanese"], required=True)
    ap.add_argument("--proxy", default="")
    ap.add_argument("--workers", type=int, default=2)
    ap.add_argument("--gap", type=float, default=0.8)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    GAP = max(0.1, args.gap)
    if args.proxy:
        _proxies = {"http": args.proxy, "https": args.proxy}

    if args.lang == "english":
        where = "status='ok' AND (name_en IS NULL OR trim(name_en)='')"
        target_col = "name_en"
    elif args.lang == "japanese":
        # 日文名单列一列（0.6 要做日文界面）。只取 NULL：空串 = 查过但没有日文名。
        where = "status IN ('ok','en_only') AND name_ja IS NULL"
        target_col = "name_ja"
    else:
        # 繁中单独一列（name_zh 是简体，绝不能被覆盖 —— 界面要做繁体模式）
        # 只取 NULL（从没查过）。空串 = 查过但没有繁体名，不再重试，
        # 否则每次重启都从第一批"没有繁中"的游戏开始重复请求（实测空转一晚）。
        where = "status IN ('ok','en_only') AND name_zh_hant IS NULL"
        target_col = "name_zh_hant"

    conn = sqlite3.connect(DB, timeout=30)
    conn.execute("PRAGMA busy_timeout = 30000")
    targets = [str(a) for (a,) in conn.execute(f"SELECT steam_appid FROM steam_names WHERE {where}")]
    if args.limit:
        targets = targets[: args.limit]

    print(f"通道 {args.lang} → 写 {target_col}；待补 {len(targets):,} 条")
    if args.dry_run:
        print("  [dry-run] 前 5 个 appid:", targets[:5])
        conn.close()
        return 0
    if not targets:
        conn.close()
        return 0

    started = time.time()
    buf: list[tuple[str, str]] = []
    raw_buf: list[tuple[str, bytes, str]] = []

    def flush() -> None:
        if not buf and not raw_buf:
            return
        if buf:
            conn.executemany(
                f"UPDATE steam_names SET {target_col} = ? WHERE steam_appid = ?",
                [(name, appid) for name, appid in buf],
            )
        if raw_buf:
            conn.executemany(
                "INSERT INTO steam_raw(steam_appid, payload, fetched_at) VALUES(?, ?, ?) "
                "ON CONFLICT(steam_appid) DO UPDATE SET "
                "payload = excluded.payload, fetched_at = excluded.fetched_at",
                raw_buf,
            )
        conn.commit()
        buf.clear()
        raw_buf.clear()

    from concurrent.futures import ThreadPoolExecutor, as_completed

    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = {pool.submit(fetch, a, args.lang): a for a in targets}
        for done, future in enumerate(as_completed(futures), 1):
            appid = futures[future]
            data = future.result()
            name = str((data or {}).get("name") or "").strip() or None
            # 原始响应一律入库，不管这个语言拿没拿到名字
            if data:
                raw_buf.append(
                    (
                        appid,
                        gzip.compress(json.dumps(data, ensure_ascii=False).encode("utf-8"), 6),
                        datetime.now().isoformat(timespec="seconds"),
                    )
                )
                with _stats_lock:
                    _stats["raw"] += 1
            if not name:
                with _stats_lock:
                    _stats["no_name"] += 1
            else:
                # 英文通道：拿到含汉字/假名的结果一律丢弃（Steam 有些"英文名"就是中文）
                if args.lang == "english" and (CJK.search(name) or KANA.search(name)):
                    with _stats_lock:
                        _stats["junk"] += 1
                # 繁中通道：结果必须含汉字，否则说明该游戏没有繁中。
                # 写空串当"已查过、无繁体名"的标记，避免下次重启重复请求。
                elif args.lang == "tchinese" and not CJK.search(name):
                    buf.append(("", appid))
                    with _stats_lock:
                        _stats["junk"] += 1
                # 日文通道：日文名通常含假名或汉字；纯拉丁说明没有日文版，
                # 写空串标记"查过、无日文名"，避免反复重试。
                elif args.lang == "japanese" and not (KANA.search(name) or CJK.search(name)):
                    buf.append(("", appid))
                    with _stats_lock:
                        _stats["junk"] += 1
                else:
                    buf.append((name, appid))
                    with _stats_lock:
                        _stats["written"] += 1
            if len(buf) >= 20 or len(raw_buf) >= 20:
                flush()
            if done % 200 == 0:
                rate = done / (time.time() - started)
                eta = (len(targets) - done) / rate / 60 if rate else 0
                print(
                    f"  {done}/{len(targets)}  {rate:.2f}/s  "
                    f"写入 {_stats['written']} 原始 {_stats['raw']} 跳过 {_stats['no_name']} 丢弃 {_stats['junk']}  "
                    f"预计剩余 {eta:.0f} 分钟",
                    flush=True,
                )
    flush()
    conn.close()
    print(
        f"完成：{len(targets):,} 条，用时 {(time.time() - started) / 60:.1f} 分钟；"
        f"写入 {_stats['written']}，原始响应 {_stats['raw']}，无结果 {_stats['no_name']}，丢弃(非目标语言) {_stats['junk']}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
