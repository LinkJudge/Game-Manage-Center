"""繁中补采 · 自动轮换节点重跑。

逻辑：
  按延迟排序所有节点 → 依次切换 → 每个节点跑一小批（默认 40 条）
  → 看"待采"有没有下降 → 下降了就继续用这个节点，没下降就换下一个
  → 全部节点都不行就停下，最后无条件把 Steam 路由切回 DIRECT。

为什么按批而不是一次性跑完：
  被限流的节点会持续 429，跑久了只是空转。按批测能快速判断"这个节点有没有用"。
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import sqlite3

from clash_ctl import Clash

ROOT = Path(__file__).resolve().parents[2]
DB = ROOT / "src-tauri" / "resources" / "database.db"
PY = sys.executable
GROUP = "🚀 节点选择"
STEAM_URL = "https://store.steampowered.com/api/appdetails?appids=730&l=english&cc=us"
SKIP = {"DIRECT", "REJECT", "GLOBAL", "PASS", "REJECT-DROP"}


def log(msg: str) -> None:
    print(f"[{time.strftime('%H:%M:%S')}] {msg}", flush=True)


def pending() -> int:
    c = sqlite3.connect(f"file:{DB.as_posix()}?mode=ro", uri=True)
    n = c.execute(
        "SELECT COUNT(*) FROM steam_names "
        "WHERE status IN ('ok','en_only') AND name_zh_hant IS NULL"
    ).fetchone()[0]
    c.close()
    return n


def set_steam(clash: Clash, target: str) -> None:
    getattr(clash, "switch")("Steam", target)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--batch", type=int, default=40, help="每个节点先试多少条")
    ap.add_argument("--workers", type=int, default=6)
    ap.add_argument("--gap", type=float, default=0.2)
    args = ap.parse_args()

    clash = Clash()
    start_pending = pending()
    log(f"待采 {start_pending} 条")

    # Steam 分组指向节点选择，之后只切"节点选择"里的具体节点
    set_steam(clash, GROUP)

    nodes = [n for n in clash.nodes(GROUP) if n not in SKIP]
    log(f"候选节点 {len(nodes)} 个，按延迟排序…")
    scored = []
    for n in nodes:
        d = clash.delay(n, STEAM_URL, 4000)
        if d and d > 0:
            scored.append((d, n))
    scored.sort()
    log("可用节点: " + ", ".join(f"{n}({d}ms)" for d, n in scored[:8]) + (" …" if len(scored) > 8 else ""))

    if not scored:
        log("没有可用节点，退出")
        set_steam(clash, "DIRECT")
        return 1

    for idx, (delay, node) in enumerate(scored, 1):
        left = pending()
        if left == 0:
            log("已全部采完")
            break
        log(f"--- [{idx}/{len(scored)}] 切到「{node}」({delay}ms)，剩 {left} 条 ---")
        try:
            clash.switch(GROUP, node)
        except Exception as e:  # noqa: BLE001
            log(f"    切换失败：{e}")
            continue
        time.sleep(1)

        before = pending()
        cmd = [
            PY, "-u", str(ROOT / "scripts" / "zh-names" / "fetch_missing_names.py"),
            "--lang", "tchinese", "--proxy", "http://127.0.0.1:7897",
            "--workers", str(args.workers), "--gap", str(args.gap),
            "--limit", str(args.batch),
        ]
        rc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace").returncode
        after = pending()
        gained = before - after
        log(f"    本批：{before} → {after}（进账 {gained}，退出码 {rc}）")

        # 有进账就继续用同一节点，直到采完或不再进账
        while gained > 0 and after > 0:
            before = after
            subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
            after = pending()
            gained = before - after
            log(f"    继续：{before} → {after}（进账 {gained}）")
        if after == 0:
            log("已全部采完")
            break

    final = pending()
    log(f"结束：剩余 {final} 条（开始 {start_pending}）")
    set_steam(clash, "DIRECT")
    log("Steam 路由已切回 DIRECT")
    return 0


if __name__ == "__main__":
    sys.exit(main())
