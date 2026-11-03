#!/usr/bin/env python3
"""Steam 采集看门狗：节点被限流就自动换，长时间没进展就换。

背景：Steam 对同一出口 IP 会限流（429 / TLS 断连），采集器自己只会把间隔抬高，
抬到 2.0 秒后就龟速前进。这个看门狗盯着日志：
  - 5 分钟一查，新增 429 达到阈值 → 切到分组里下一个可用节点
  - 15 分钟没有新进展（数据库行数不动）→ 也切一次
切换有冷却（默认 10 分钟），不会来回抖。

用法：
  python scripts/zh-names/steam_watchdog.py                    # 前台跑
  python scripts/zh-names/steam_watchdog.py --interval 300 --group "🚀 节点选择"
"""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
import sys
import time
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STATE_DIR = ROOT.parent / ".workbuddy"
DB = ROOT / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"
DEFAULT_LOG = STATE_DIR / "steam-names-run.log"
STATE_FILE = STATE_DIR / "steam-watchdog.json"
ACTION_LOG = STATE_DIR / "steam-watchdog.log"

sys.path.insert(0, str(Path(__file__).resolve().parent))
from clash_ctl import Clash  # noqa: E402


def log(message: str) -> None:
    line = f"[{datetime.now():%Y-%m-%d %H:%M:%S}] {message}"
    print(line, flush=True)
    with ACTION_LOG.open("a", encoding="utf-8") as fh:
        fh.write(line + "\n")


def count_429(path: Path) -> int:
    if not path.exists():
        return 0
    text = path.read_text(encoding="utf-8", errors="replace")
    return text.count("429 限流")


def count_rows() -> int:
    conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        return conn.execute("SELECT COUNT(*) FROM steam_names").fetchone()[0]
    finally:
        conn.close()


def load_state() -> dict:
    if STATE_FILE.exists():
        try:
            return json.loads(STATE_FILE.read_text(encoding="utf-8"))
        except Exception:  # noqa: BLE001
            pass
    return {}


def save_state(state: dict) -> None:
    STATE_FILE.write_text(json.dumps(state, ensure_ascii=False, indent=1), encoding="utf-8")


def rotate(clash: Clash, group: str, url: str) -> str | None:
    """切到分组里下一个测速通过的节点，返回新节点名。"""
    nodes = clash.nodes(group)
    current = clash.current(group)
    if not nodes:
        return None
    index = nodes.index(current) if current in nodes else -1
    for step in range(1, len(nodes) + 1):
        candidate = nodes[(index + step) % len(nodes)]
        delay = clash.delay(candidate, url, 6000)
        if delay:
            clash.switch(group, candidate)
            return f"{candidate} ({delay}ms)"
        log(f"  跳过 {candidate}：测速失败")
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description="Steam 采集看门狗")
    ap.add_argument("--interval", type=int, default=300, help="检查间隔秒，默认 300")
    ap.add_argument("--group", default="🚀 节点选择", help="要切换的 Clash 分组")
    ap.add_argument("--log", type=Path, default=DEFAULT_LOG, help="采集日志路径")
    ap.add_argument("--url", default="https://store.steampowered.com/api/appdetails?appids=730")
    ap.add_argument("--cooldown", type=int, default=600, help="两次切换的最小间隔秒")
    ap.add_argument("--stall", type=int, default=900, help="多久没进展算卡住（秒）")
    ap.add_argument("--max-rotations", type=int, default=200, help="最多切多少次后退出")
    args = ap.parse_args()

    STATE_DIR.mkdir(parents=True, exist_ok=True)
    state = load_state()
    last_429 = count_429(args.log)
    last_rows = count_rows()
    last_progress = time.time()
    rotations = 0

    log(f"看门狗启动：间隔 {args.interval}s，分组「{args.group}」，日志 {args.log.name}")
    log(f"  当前 429 计数 {last_429}，数据库 {last_rows} 行")

    try:
        clash = Clash()
    except Exception as error:  # noqa: BLE001
        log(f"[致命] 连不上 Clash：{error}")
        return 1

    while True:
        time.sleep(args.interval)
        try:
            now_429 = count_429(args.log)
            now_rows = count_rows()
            now = time.time()
            new_429 = now_429 - last_429
            grew = now_rows - last_rows

            if grew > 0:
                last_progress = now
                last_rows = now_rows
            last_429 = now_429

            due_429 = new_429 >= 2
            due_stall = (now - last_progress) >= args.stall
            if not (due_429 or due_stall):
                if grew:
                    log(f"正常：+{grew} 行（总 {now_rows}），新增 429 {new_429} 次")
                continue

            reason = "429 增多" if due_429 else f"{int((now - last_progress) // 60)} 分钟无进展"
            since_last = now - state.get("last_rotate_ts", 0)
            if since_last < args.cooldown:
                log(f"检测到{reason}，但距上次切换仅 {int(since_last)}s，等冷却")
                continue

            old = clash.current(args.group)
            result = rotate(clash, args.group, args.url)
            if result:
                rotations += 1
                state["last_rotate_ts"] = now
                state["rotations"] = state.get("rotations", 0) + 1
                save_state(state)
                log(f"检测到{reason} → 「{args.group}」{old} 换成 {result}（第 {rotations} 次）")
                last_progress = now  # 换完重新计时，别立刻又触发
            else:
                log(f"检测到{reason}，但分组里没有可用节点")
                last_progress = now
        except Exception as error:  # noqa: BLE001 - 看门狗自己不能死
            log(f"[异常] {type(error).__name__}: {error}")


if __name__ == "__main__":
    sys.exit(main())
