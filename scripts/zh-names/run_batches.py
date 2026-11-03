#!/usr/bin/env python3
"""按批续跑小黑盒中文名采集（xiaoheihe_zh.py 的外层调度器）。

为什么要外层调度：小黑盒是内部接口，长期连续请求必然触发风控。
单批小、批间冷却、一遇风控立刻停手，比一次跑到底安全得多。

示例：
  python scripts/zh-names/run_batches.py                       # 默认 4 批 × 400 请求
  python scripts/zh-names/run_batches.py --batches 8 --budget 250 --gap 600
  python scripts/zh-names/run_batches.py --probe                # 只跑 1 批 × 20，探活

退出码：0 正常结束（跑完指定批数或待采清空）；2 被风控熔断；3 处于冷却期。
"""

from __future__ import annotations

import argparse
import sqlite3
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path
import json
from collection_state import preflight, validate_target

# 控制台编码兜底：计划任务/无 PYTHONUTF8 环境下 stdout 默认是 GBK，
# 碰到游戏名里的 ™ ® 会直接 UnicodeEncodeError 崩掉（2026-11-02 实测）。
for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(encoding="utf-8", errors="replace")
    except Exception:  # noqa: BLE001
        pass

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = Path(__file__).resolve().parent / "xiaoheihe_zh.py"
DB = ROOT / "scripts" / "pcgw" / "database" / "gmc-dev-full.db"
STATE_DIR = ROOT.parent / ".workbuddy"


def log_path() -> Path:
    return STATE_DIR / f"zh-names-batches-{datetime.now():%Y%m%d-%H%M}.log"


def stats() -> tuple[int, int, int]:
    """(已记录来源数, 已有中文名数, 待采数)"""
    conn = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    try:
        sources = conn.execute("SELECT COUNT(*) FROM name_sources WHERE source='xiaoheihe'").fetchone()[0]
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


def main() -> int:
    global DB
    parser = argparse.ArgumentParser(description="小黑盒中文名采集 · 分批调度")
    parser.add_argument("--batches", type=int, default=4, help="跑几批（默认 4）")
    parser.add_argument("--budget", type=int, default=400, help="每批请求数（默认 400；2026-11-03 提速）")
    parser.add_argument("--gap", type=int, default=300, help="批间冷却秒数（默认 300；2026-11-03 提速）")
    parser.add_argument("--hot", default="", help="优先榜单文件（steam-priority.json），按榜上名次优先采")
    parser.add_argument("--hot-top", type=int, default=0, help="只采榜单前 N 名（0=榜单内全部待采）")
    parser.add_argument("--probe", action="store_true", help="探活模式：1 批 × 20 请求")
    parser.add_argument("--db", type=Path, default=DB)
    parser.add_argument("--preflight", action="store_true", help="只读自检，不联网")
    args = parser.parse_args()
    global_db = validate_target(args.db)
    DB = global_db
    if args.preflight:
        print(json.dumps(preflight(global_db), ensure_ascii=False, indent=2))
        return 0

    batches, budget = (1, 20) if args.probe else (args.batches, args.budget)
    if budget < 1 or batches < 1:
        print("[错误] --budget / --batches 必须为正数")
        return 1

    STATE_DIR.mkdir(parents=True, exist_ok=True)
    log = log_path()
    sources0, zh0, pending0 = stats()
    started = time.time()

    def emit(line: str) -> None:
        print(line, flush=True)
        with log.open("a", encoding="utf-8") as fh:
            fh.write(line + "\n")

    emit(f"=== 开始 {datetime.now():%Y-%m-%d %H:%M:%S} ===")
    emit(f"起始：来源记录 {sources0}，中文名 {zh0}，待采 {pending0}")
    emit(f"计划：{batches} 批 × {budget} 请求，批间冷却 {args.gap}s；日志 {log.name}")

    for i in range(1, batches + 1):
        # 每批都重新数一次：上一批已落库，剩余量随之下降
        _, _, pending = stats()
        if pending == 0:
            emit("待采已清空，收工。")
            break

        emit(f"--- 第 {i}/{batches} 批 开始 {datetime.now():%H:%M:%S}（待采 {pending}）---")
        cmd = [
            sys.executable, "-u", str(SCRIPT),
            "--budget", str(budget),
            "--limit", str(budget),
            "--db", str(DB),
            "--min-delay", "2.2", "--max-delay", "5.5",
            "--pause-every", "26", "48", "--pause-range", "40", "110",
        ]
        if args.hot:
            cmd += ["--hot", str((ROOT / args.hot).resolve() if not Path(args.hot).is_absolute() else args.hot)]
            if args.hot_top > 0:
                cmd += ["--hot-top", str(args.hot_top)]
        process = subprocess.run(
            cmd, cwd=str(ROOT), capture_output=True, text=True, encoding="utf-8", errors="replace"
        )
        tail = [ln for ln in (process.stdout or "").splitlines() if ln.strip()]
        for line in tail[-6:]:
            emit("   " + line)

        code = process.returncode
        sources1, zh1, pending1 = stats()
        emit(
            f"第 {i} 批结束 exit={code}：来源 {sources0}→{sources1}（+{sources1 - sources0}），"
            f"中文名 {zh0}→{zh1}（+{zh1 - zh0}），待采 {pending1}"
        )

        if code in (2, 3, 4):
            emit("触发了风控/冷却/预算或单进程保护，立即停止。已采集的数据都已落库，冷却后再跑。")
            return code
        if code != 0:
            err = (process.stderr or "").strip().splitlines()
            emit(f"异常退出，最后一条错误：{err[-1] if err else '(无)'}")
            return code

        if i < batches and pending1 > 0:
            emit(f"冷却 {args.gap}s ……")
            time.sleep(args.gap)

    sources1, zh1, pending1 = stats()
    emit(
        f"=== 全部结束 {datetime.now():%Y-%m-%d %H:%M:%S} 耗时 "
        f"{(time.time() - started) / 60:.1f} 分钟 ==="
    )
    emit(
        f"本轮净增：来源记录 +{sources1 - sources0}，中文名 +{zh1 - zh0}；"
        f"中文名总数 {zh1}，剩余待采 {pending1}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
