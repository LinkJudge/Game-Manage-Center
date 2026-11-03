#!/usr/bin/env python3
"""小黑盒采集 · 一句话状态检查。

为什么需要它：采集跑在 Windows 计划任务里，跑完不会给我们推通知。
这个脚本只做只读检查，靠「数据库写入时间」判断活死，不依赖进程 API。

用法：
  python scripts/zh-names/status.py
  python scripts/zh-names/status.py --json     # 给自动化/脚本消费
"""

from __future__ import annotations

import argparse
import json
import sqlite3
import sys
import time
from datetime import datetime
from pathlib import Path
from collection_state import collector_running, preflight

ROOT = Path(__file__).resolve().parents[2]
DB = ROOT / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"
STATE = ROOT.parent / ".workbuddy"


def active_log() -> Path | None:
    """取最新的采集日志：run_batches 写 zh-names-batches-*.log、后台任务写
    zh-run-offlist*.log，计划任务写 zh-collect.log；按修改时间取最新。"""
    cands = [
        p
        for p in list(STATE.glob("zh-names-batches-*.log"))
        + list(STATE.glob("zh-run-offlist*.log"))
        + list(STATE.glob("zh-collect.log"))
        if p.exists()
    ]
    return max(cands, key=lambda p: p.stat().st_mtime) if cands else None

# 数据库每 3 分钟左右落一次；超过这个秒数没动静就认为不在写
STALE_WARN = 600
STALE_BAD = 1800


def counts() -> tuple[int, int, int]:
    conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        sources = conn.execute(
            "SELECT COUNT(*) FROM name_sources WHERE source='xiaoheihe'"
        ).fetchone()[0]
        zh = conn.execute(
            "SELECT COUNT(*) FROM games WHERE name_zh IS NOT NULL AND trim(name_zh) <> ''"
        ).fetchone()[0]
        pending = conn.execute(
            """
            SELECT COUNT(*) FROM games g
            LEFT JOIN name_sources n ON n.pcgw_id = g.pcgw_id AND n.source = 'xiaoheihe'
            WHERE g.steam_appid IS NOT NULL AND trim(CAST(g.steam_appid AS TEXT)) <> ''
              AND (g.name_zh IS NULL OR trim(g.name_zh) = '')
              AND n.pcgw_id IS NULL
            """
        ).fetchone()[0]
        return sources, zh, pending
    finally:
        conn.close()


def tail(path: Path, n: int = 8) -> list[str]:
    if not path.exists():
        return []
    with path.open("r", encoding="utf-8", errors="replace") as fh:
        return [ln.rstrip("\n") for ln in fh.readlines()[-n:]]


def steam_progress() -> dict:
    """Steam 官方采集（中英文）的进度。"""
    # 日志名随启动方式变：计划任务写 steam-names.log，后台任务写 steam-names-run.log。
    # 取最新的那份，否则会一直显示几小时前的旧日志。
    logs = [p for p in STATE.glob("steam-names*.log") if p.exists()]
    log = max(logs, key=lambda p: p.stat().st_mtime) if logs else None
    conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        done = conn.execute("SELECT COUNT(*) FROM steam_names").fetchone()[0]
        has_en = conn.execute(
            "SELECT COUNT(*) FROM steam_names WHERE name_en IS NOT NULL AND trim(name_en)<>''"
        ).fetchone()[0]
        targets = conn.execute(
            "SELECT COUNT(DISTINCT steam_appid) FROM games "
            "WHERE steam_appid IS NOT NULL AND trim(CAST(steam_appid AS TEXT)) <> ''"
        ).fetchone()[0]
    finally:
        conn.close()
    idle = int(time.time() - log.stat().st_mtime) if log else -1
    tail_lines = tail(log, 3) if log else []
    return {
        "done": done,
        "has_en": has_en,
        "total": targets,
        "pending": max(0, targets - done),
        "log": log.name if log else None,
        "log_idle_seconds": idle,
        "log_tail": tail_lines,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="采集状态检查")
    ap.add_argument("--json", action="store_true", help="输出 JSON")
    ap.add_argument("--tail", type=int, default=8, help="日志尾部行数")
    args = ap.parse_args()

    now = time.time()
    # WAL 模式下写入先进 -wal 文件，主 db 的 mtime 长时间不动；
    # 取两者最新的才算「最后落库」。
    mtime = 0.0
    for p in (DB, Path(str(DB) + "-wal")):
        if p.exists():
            mtime = max(mtime, p.stat().st_mtime)
    idle = int(now - mtime) if mtime else -1
    log = active_log()
    lines = tail(log, args.tail) if log else []
    sources, zh, pending = counts()

    finished = any("=== 全部结束" in ln or "EXIT_CODE=" in ln for ln in lines)
    triggered = any("触发了风控" in ln for ln in lines)

    active = collector_running()
    if active:
        state, hint = "运行中", "采集进程持有独占锁"
    elif triggered:
        state, hint = "风控熔断", "已被拦，等待冷却后再启动，别重试"
    elif finished:
        state, hint = "已结束", "本轮跑完了，可以起下一轮"
    elif idle < 0:
        state, hint = "无数据", "数据库不存在，检查路径"
    elif idle <= STALE_WARN:
        state, hint = "运行中", "正常，数据库在落"
    elif idle <= STALE_BAD:
        state, hint = "可疑", "10~30 分钟没落库，可能在长冷却或卡住"
    else:
        state, hint = "疑似停止", "超过 30 分钟没落库，进程可能没了，需要重启"

    data = {
        "database": str(DB),
        "collector_active": active,
        "preflight": preflight(DB),
        "checked_at": datetime.now().strftime("%Y-%m-%d %H:%M:%S"),
        "state": state,
        "hint": hint,
        "log": log.name if log else None,
        "db_idle_seconds": idle,
        "db_last_write": datetime.fromtimestamp(mtime).strftime("%Y-%m-%d %H:%M:%S") if mtime else None,
        "sources": sources,
        "zh_names": zh,
        "pending": pending,
        "log_tail": lines,
        "steam": steam_progress(),
    }

    if args.json:
        print(json.dumps(data, ensure_ascii=False, indent=1))
        return 0

    print(f"检查时间   {data['checked_at']}")
    print(f"结论       {state} —— {hint}")
    print(f"日志       {data['log'] or '（未找到）'}")
    print(f"最后落库   {data['db_last_write']}（{idle // 60} 分钟前）")
    print(f"来源记录   {sources:,}")
    print(f"中文名     {zh:,}")
    print(f"待采       {pending:,}")
    st = data["steam"]
    pct = st["done"] / st["total"] * 100 if st["total"] else 0
    print()
    print(f"Steam 官方 {st['done']:,} / {st['total']:,}（{pct:.1f}%），有英文名 {st['has_en']:,}，待采 {st['pending']:,}")
    if st["log_idle_seconds"] >= 0:
        print(f"  {st['log']} {st['log_idle_seconds'] // 60} 分钟前更新")
    for ln in st["log_tail"]:
        print(f"  {ln}")
    print()
    print("日志尾部   " + ("（无）" if not lines else ""))
    for ln in lines:
        print(f"  {ln}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
