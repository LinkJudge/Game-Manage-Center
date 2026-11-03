#!/usr/bin/env python3
"""繁中名字批量补采作业：切代理 → 采集 → 无论成败都切回 DIRECT。

为什么单独写：Clash 分组名带 emoji（🚀 节点选择），不能写进 .cmd；而且
「切路由」和「恢复路由」必须配对，出错也要恢复，否则用户 Steam 会被挂代理。

用法：
  python scripts/zh-names/run_hant_job.py                # 采繁中（默认）
  python scripts/zh-names/run_hant_job.py --dry-run      # 只看计划不执行
"""

from __future__ import annotations

import argparse
import ctypes
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STATE = ROOT.parent / ".workbuddy"
LOG = STATE / "ja-job.log"
PY = sys.executable
PROXY = "http://127.0.0.1:7897"
GROUP_STEAM = "Steam"
GROUP_NODE = "🚀 节点选择"

sys.path.insert(0, str(Path(__file__).resolve().parent))
from clash_ctl import Clash  # noqa: E402


def log(msg: str) -> None:
    line = f"[{datetime.now():%Y-%m-%d %H:%M:%S}] {msg}"
    print(line, flush=True)
    with LOG.open("a", encoding="utf-8") as fh:
        fh.write(line + "\n")


def set_route(clash: Clash, node: str) -> None:
    clash.switch(GROUP_STEAM, node)
    log(f"路由：{GROUP_STEAM} -> {node}")


def main() -> int:
    ap = argparse.ArgumentParser(description="繁中名补采作业")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--gap", type=float, default=0.18)
    args = ap.parse_args()

    STATE.mkdir(parents=True, exist_ok=True)

    # 单实例互斥：计划任务被重复触发时会起多个作业，它们抢同一个代理和数据库，
    # 结果不是更快而是更慢（而且日志会互相覆盖）。用命名互斥体挡掉后来的。
    # 判断依据：CreateMutexW 对已存在的名字返回有效句柄，但 GetLastError()==183；
    # ctypes 的 GetLastError 必须紧跟调用取（ctypes.get_last_error 配 use_last_error=True 更稳，
    # 这里 windll 默认模式，直接 kernel32.GetLastError() 也在同一线程、紧跟调用，实测可靠）。
    kernel32 = ctypes.windll.kernel32
    kernel32.CreateMutexW.restype = ctypes.c_void_p
    _mutex = kernel32.CreateMutexW(None, False, "Global\\gm_ja_job")
    if kernel32.GetLastError() == 183:  # ERROR_ALREADY_EXISTS
        log("已有繁中作业在运行，本实例退出")
        return 0

    log("=" * 60)
    log("日文名补采作业启动")

    clash = Clash()
    try:
        set_route(clash, GROUP_NODE)
        if args.dry_run:
            log("[dry-run] 不做实际采集")
            return 0
        cmd = [
            PY, "-u", str(ROOT / "scripts" / "zh-names" / "fetch_missing_names.py"),
            "--lang", "japanese", "--proxy", PROXY,
            "--workers", str(args.workers), "--gap", str(args.gap),
        ]
        log("开始采集：" + " ".join(cmd[1:]))
        started = time.time()
        code = subprocess.call(cmd)
        log(f"采集结束 exit={code}，用时 {(time.time() - started) / 60:.1f} 分钟")
        return code
    finally:
        # 无论成功、失败还是异常，都把路由恢复成直连
        try:
            set_route(clash, "DIRECT")
            log("已恢复 Steam 直连路由")
        except Exception as error:  # noqa: BLE001
            log(f"[严重] 恢复路由失败：{error} —— 请手动执行 clash_ctl.py set --group Steam --node DIRECT")


if __name__ == "__main__":
    sys.exit(main())
