#!/usr/bin/env python3
"""Clash（Verge Rev / mihomo）节点控制：列出、测试、切换。

为什么走命名管道：Clash Verge Rev 默认不开放 TCP 控制端口（`external-controller: ''`），
只留 `external-controller-pipe`。Python 直接对管道读写 HTTP 报文即可，不需要额外依赖。

用法：
  python scripts/zh-names/clash_ctl.py list                  # 看分组与当前节点
  python scripts/zh-names/clash_ctl.py test --group 节点选择  # 测所有节点对 Steam 的延迟
  python scripts/zh-names/clash_ctl.py rotate --group 节点选择  # 切到下一个可用节点
  python scripts/zh-names/clash_ctl.py pick --group 节点选择 --url https://store.steampowered.com
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.parse
from pathlib import Path

CFG = Path(os.environ.get("APPDATA", "")) / "io.github.clash-verge-rev.clash-verge-rev" / "clash-verge.yaml"
CFG2 = CFG.with_name("config.yaml")


def _dechunk(data: bytes) -> bytes:
    """解 chunked 传输编码。/proxies 这种大响应 mihomo 会用 chunked。"""
    out = b""
    while data:
        line, _, data = data.partition(b"\r\n")
        if not line:
            break
        try:
            size = int(line.split(b";")[0], 16)
        except ValueError:
            break
        if size == 0:
            break
        out += data[:size]
        data = data[size + 2 :]
    return out


def resolve_pipe() -> str:
    """管道名会随 Clash 重启变化，直接枚举 \\\\.\\pipe\\ 找 verge-mihomo*。"""
    for name in os.listdir(r"\\.\pipe\\"):
        if name.startswith("verge-mihomo"):
            return "\\\\.\\pipe\\" + name
    raise RuntimeError("找不到 verge-mihomo 管道，Clash 可能没在跑")


def resolve_secret() -> str:
    for path in (CFG2, CFG):
        if not path.exists():
            continue
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("secret:"):
                value = line.split(":", 1)[1].strip().strip("'\"")
                if value:
                    return value
    return ""


class Clash:
    def __init__(self) -> None:
        self.pipe = resolve_pipe()
        self.secret = resolve_secret()

    def call(self, path: str, method: str = "GET", body: dict | None = None) -> dict:
        head = (
            f"{method} {path} HTTP/1.1\r\nHost: localhost\r\n"
            f"Authorization: Bearer {self.secret}\r\nConnection: close\r\n"
        )
        data = b""
        if body is not None:
            data = json.dumps(body).encode()
            head += f"Content-Type: application/json\r\nContent-Length: {len(data)}\r\n"
        head += "\r\n"
        with open(self.pipe, "r+b", buffering=0) as f:
            f.write(head.encode() + data)
            raw = b""
            while True:
                chunk = f.read(16384)
                if not chunk:
                    break
                raw += chunk

        head_bytes, _, rest = raw.partition(b"\r\n\r\n")
        header_text = head_bytes.decode("latin-1")
        status = int(header_text.split(" ", 2)[1]) if header_text.startswith("HTTP/") else 0
        if "transfer-encoding: chunked" in header_text.lower():
            body = _dechunk(rest)
        else:
            body = rest
        body_text = body.decode("utf-8", "replace")
        if status >= 400:
            raise RuntimeError(f"HTTP {status}: {body_text[:200]}")
        return json.loads(body_text) if body_text.strip() else {}

    def groups(self) -> dict[str, dict]:
        proxies = self.call("/proxies").get("proxies", {})
        return {
            name: info
            for name, info in proxies.items()
            if info.get("type") in ("Selector", "URLTest", "Fallback", "LoadBalance")
        }

    def nodes(self, group: str) -> list[str]:
        return list(self.call("/proxies").get("proxies", {}).get(group, {}).get("all", []))

    def current(self, group: str) -> str:
        return self.call("/proxies").get("proxies", {}).get(group, {}).get("now", "")

    def switch(self, group: str, node: str) -> None:
        self.call(f"/proxies/{urllib.parse.quote(group)}", "PUT", {"name": node})

    def delay(self, node: str, url: str, timeout: int = 5000) -> int | None:
        query = urllib.parse.urlencode({"url": url, "timeout": timeout})
        try:
            data = self.call(f"/proxies/{urllib.parse.quote(node)}/delay?{query}")
            return int(data.get("delay", -1))
        except Exception:
            return None


def main() -> int:
    ap = argparse.ArgumentParser(description="Clash 节点控制")
    ap.add_argument("action", choices=["list", "test", "rotate", "pick", "set"])
    ap.add_argument("--group", default="", help="分组名（默认取第一个 Selector）")
    ap.add_argument("--node", default="", help="set 动作的目标节点名")
    ap.add_argument("--url", default="https://store.steampowered.com/api/appdetails?appids=730",
                    help="测速用的目标 URL")
    ap.add_argument("--timeout", type=int, default=5000, help="测速超时毫秒")
    args = ap.parse_args()

    c = Clash()
    groups = c.groups()
    if not groups:
        print("没有找到可切换的分组")
        return 1

    group = args.group
    if not group:
        group = next((n for n, i in groups.items() if i.get("type") == "Selector"), "")
        if not group:
            print("没有 Selector 类型的分组，可选：", list(groups))
            return 1

    if args.action == "list":
        for name, info in groups.items():
            print(f"[{info.get('type')}] {name}  ->  {info.get('now')}  ({len(info.get('all', []))} 个节点)")
        return 0

    nodes = c.nodes(group)
    current = c.current(group)

    if args.action == "set":
        if not args.node:
            print("set 需要 --node；可选：", nodes)
            return 1
        if args.node not in nodes:
            print(f"「{args.node}」不在分组「{group}」里；可选：{nodes}")
            return 1
        if args.node == current:
            print(f"「{group}」已经是「{args.node}」，无需切换")
            return 0
        c.switch(group, args.node)
        print(f"「{group}」：{current} -> {args.node}")
        return 0

    if args.action == "test":
        print(f"分组「{group}」当前：{current}；测速目标 {args.url}")
        results = []
        for node in nodes:
            d = c.delay(node, args.url, args.timeout)
            results.append((d if d else 10**9, node))
            print(f"  {'✗' if d is None else str(d) + 'ms':>8}  {node}")
        results.sort()
        print("\n最快三个：")
        for d, node in results[:3]:
            print(f"  {d if d < 10**9 else '超时'}  {node}")
        return 0

    if args.action == "rotate":
        if not nodes:
            print("分组里没有节点")
            return 1
        idx = nodes.index(current) if current in nodes else -1
        for step in range(1, len(nodes) + 1):
            nxt = nodes[(idx + step) % len(nodes)]
            d = c.delay(nxt, args.url, args.timeout)
            if d:
                c.switch(group, nxt)
                print(f"已切换到「{nxt}」（{d}ms），原节点「{current}」")
                return 0
            print(f"  跳过「{nxt}」：测速失败")
        print("所有节点都不通，保持原样")
        return 1

    if args.action == "pick":
        best, best_delay = None, 10**9
        for node in nodes:
            d = c.delay(node, args.url, args.timeout)
            if d and d < best_delay:
                best, best_delay = node, d
        if not best:
            print("没有可用节点")
            return 1
        if best != current:
            c.switch(group, best)
            print(f"已切到最快节点「{best}」（{best_delay}ms），原「{current}」")
        else:
            print(f"当前已是最快节点「{best}」（{best_delay}ms）")
        return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
