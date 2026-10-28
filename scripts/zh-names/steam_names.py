#!/usr/bin/env python3
"""从 Steam 官方商店抓游戏名（简体中文 + 英文），作为主名的第一优先级来源。

优先级约定（戢翎 2026-10-28 定）：官方 > 小黑盒 > PCGamingWiki。
本脚本只负责「官方」这一档，结果写入独立表，不动现有主名。

写入 `game_name_official`：
  steam_appid   TEXT PRIMARY KEY
  name_zh       TEXT    -- l=schinese 返回的 name（可能仍是英文，见 status）
  name_en       TEXT    -- l=english 返回的 name
  status        TEXT    -- ok / en_only / missing / error
  checked_at    TEXT

⚠️ 关键陷阱（2026-10-27 实测）：Steam 对大量游戏「没有做中文本地化」，
`l=schinese` 返回的 name 仍然是英文原名（例：CS2 -> "Counter-Strike 2"）。
所以不能只看返回值非空就当成中文名，必须用 status 区分：
  ok      = 官方确实给了中文名（name_zh 含 CJK 汉字）
  en_only = 官方中文位返回的还是英文，只有英文名可用

为什么单独一张表而不是直接改 games：
- `games.title` 是 PRIMARY KEY，`zh_CN` 是被多处读取的展示字段，
  直接改会牵动匹配逻辑且难以回滚；先采齐、再一次性按优先级重建。

网络说明：本机出网走沙箱代理（http_proxy），实测时好时坏，
appdetails 常见 502 / 连接超时。脚本对 5xx、429、超时一律退避重试，
并在 cn 区查不到时回退 us 区。

用法：
  python scripts/zh-names/steam_names.py --dry-run --limit 5
  python scripts/zh-names/steam_names.py --only-hot --budget 300
  python scripts/zh-names/steam_names.py --budget 2000
  python scripts/zh-names/steam_names.py --retry-errors
"""

from __future__ import annotations

import argparse
import json
import random
import socket
import sqlite3
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DB = ROOT / "src-tauri" / "resources" / "database.db"
DEFAULT_HOT = ROOT.parent / ".workbuddy" / "steam-topsellers.json"

UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
)
API = "https://store.steampowered.com/api/appdetails?appids={appid}&l={lang}&cc={cc}"

SCHEMA = """
CREATE TABLE IF NOT EXISTS game_name_official (
    steam_appid TEXT PRIMARY KEY,
    name_zh     TEXT,
    name_en     TEXT,
    status      TEXT NOT NULL,
    checked_at  TEXT NOT NULL
)
"""

# 连续多少次网络失败后就停手，避免把 IP 玩坏 / 空转
MAX_CONSECUTIVE_NET_FAILURES = 30


def has_cjk(text: str | None) -> bool:
    """是否包含汉字（含 CJK 扩展区与常用标点不影响判定）。"""
    if not text:
        return False
    return any("一" <= ch <= "鿿" or "㐀" <= ch <= "䶿" for ch in text)


class Fetcher:
    """带重试、退避与地区回退的 appdetails 取数器。"""

    def __init__(
        self,
        min_delay: float,
        max_delay: float,
        retries: int,
        cc_order: list[str],
        timeout: int = 12,
    ):
        self.min_delay = min_delay
        self.max_delay = max_delay
        self.retries = retries
        self.cc_order = cc_order
        self.timeout = timeout
        self.net_failures = 0
        self.requests = 0

    def _sleep(self) -> None:
        time.sleep(random.uniform(self.min_delay, self.max_delay))

    def get(self, appid: str, lang: str):
        """返回 (found: bool, name|None, fatal: bool)。

        found=True  官方有条目（name 可能是 None 表示字段缺失）
        found=False 官方确认无条目（success=false）——不用再重试
        fatal=True  网络彻底失败，调用方应记 error
        """
        last_net = None
        for cc in self.cc_order:
            for attempt in range(self.retries):
                self._sleep()
                self.requests += 1
                url = API.format(appid=appid, lang=lang, cc=cc) if cc else (
                    f"https://store.steampowered.com/api/appdetails?appids={appid}&l={lang}"
                )
                request = urllib.request.Request(
                    url, headers={"User-Agent": UA, "Accept": "application/json"}
                )
                try:
                    with urllib.request.urlopen(request, timeout=self.timeout) as response:
                        payload = json.loads(response.read().decode("utf-8"))
                except urllib.error.HTTPError as error:
                    if error.code in (429, 403):
                        print(f"      ! HTTP {error.code} 限流，休息 120s")
                        time.sleep(120)
                        last_net = error
                        continue
                    if error.code >= 500:
                        wait = min(8, 2 * (attempt + 1))
                        print(f"      ! HTTP {error.code}（cc={cc or 'default'}）{wait}s 后重试")
                        time.sleep(wait)
                        last_net = error
                        continue
                    # 400 之类：换个地区再试
                    last_net = error
                    break
                except (urllib.error.URLError, socket.timeout, TimeoutError, OSError) as error:
                    wait = min(8, 2 * (attempt + 1))
                    print(f"      ! 网络 {type(error).__name__}（cc={cc or 'default'}）{wait}s 后重试")
                    time.sleep(wait)
                    last_net = error
                    continue
                except json.JSONDecodeError as error:
                    wait = min(6, 2 * (attempt + 1))
                    print(f"      ! 响应不是 JSON，{wait}s 后重试")
                    time.sleep(wait)
                    last_net = error
                    continue

                entry = payload.get(appid) or {}
                if not entry.get("success"):
                    # 官方确认没这个 appid，换地区再确认一次即可
                    last_net = None
                    break
                self.net_failures = 0
                return True, (entry.get("data") or {}).get("name"), False

        if last_net is not None:
            self.net_failures += 1
            return False, None, True
        # 所有地区都明确返回 success=false
        self.net_failures = 0
        return False, None, False


def main() -> int:
    parser = argparse.ArgumentParser(description="抓 Steam 官方中英文游戏名")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--hot", type=Path, default=DEFAULT_HOT, help="热销榜 JSON")
    parser.add_argument("--only-hot", action="store_true", help="只处理热销榜范围内的游戏")
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--budget", type=int, default=300, help="本轮最多处理多少个游戏")
    parser.add_argument("--min-delay", type=float, default=1.3)
    parser.add_argument("--max-delay", type=float, default=2.2)
    parser.add_argument("--retries", type=int, default=1)
    parser.add_argument("--timeout", type=int, default=12, help="单次请求超时（秒）")
    parser.add_argument("--retry-errors", action="store_true", help="重跑上一轮网络失败的条目")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    if not args.db.exists():
        print(f"[错误] 找不到数据库：{args.db}")
        return 1

    hot: set[str] = set()
    if args.only_hot:
        if not args.hot.exists():
            print(f"[错误] 找不到热销榜：{args.hot}")
            return 1
        hot = {str(x["appid"]) for x in json.loads(args.hot.read_text(encoding="utf-8"))}

    conn = sqlite3.connect(args.db)
    conn.execute(SCHEMA)
    conn.commit()

    done = {str(r[0]) for r in conn.execute("SELECT steam_appid FROM game_name_official")}
    if args.retry_errors:
        errors = {
            str(r[0])
            for r in conn.execute("SELECT steam_appid FROM game_name_official WHERE status='error'")
        }
        done -= errors
        print(f"重跑模式：{len(errors)} 个失败条目重新排队")

    rows = conn.execute(
        "SELECT steam_id, title FROM games "
        "WHERE steam_id IS NOT NULL AND trim(CAST(steam_id AS TEXT))<>''"
    ).fetchall()

    targets: list[str] = []
    seen: set[str] = set()
    for steam_id, _title in rows:
        digits = "".join(ch for ch in str(steam_id) if ch.isdigit())
        if not digits or digits in seen or digits in done:
            continue
        if hot and digits not in hot:
            continue
        seen.add(digits)
        targets.append(digits)
    random.shuffle(targets) if not hot else None  # 全量时打散，避免连续命中同一批被限流
    if args.limit > 0:
        targets = targets[: args.limit]

    print(
        f"库内 AppID 去重 {len(seen)} 个；已抓过 {len(done)} 个；"
        f"本轮 {len(targets)} 个，预算 {args.budget}"
    )
    if args.dry_run:
        print("[dry-run] 前 10:", targets[:10])
        conn.close()
        return 0
    if not targets:
        conn.close()
        print("没有待处理记录。")
        return 0

    fetcher = Fetcher(
        args.min_delay, args.max_delay, args.retries, ["cn", "us"], args.timeout
    )
    buf: list[tuple[str, str | None, str | None, str, str]] = []
    stats = {"ok": 0, "en_only": 0, "missing": 0, "error": 0}
    processed = 0
    started = time.time()

    def flush() -> None:
        if not buf:
            return
        conn.executemany(
            """
            INSERT INTO game_name_official(steam_appid, name_zh, name_en, status, checked_at)
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

    for index, appid in enumerate(targets, 1):
        if processed >= args.budget:
            print(f"已达预算 {args.budget}，收工。重跑会从剩余继续。")
            break
        if fetcher.net_failures >= MAX_CONSECUTIVE_NET_FAILURES:
            print(f"连续网络失败 {fetcher.net_failures} 次，停手。等网络恢复后重跑。")
            break

        processed += 1
        checked_at = datetime.now().isoformat(timespec="seconds")

        found_zh, name_zh, fatal_zh = fetcher.get(appid, "schinese")

        # 分支 1：中文请求网络失败 —— 直接记 error，不补英文请求
        # （补英文要再发 2 次，网络差时纯浪费；error 条目最后用 --retry-errors 统一补）
        if fatal_zh:
            stats["error"] += 1
            buf.append((appid, None, None, "error", checked_at))
            print(f"[{index}/{len(targets)}] {appid} -> 网络失败，跳过")
            if len(buf) >= 20:
                flush()
            continue

        # 分支 2：官方明确查不到这个 appid
        if not found_zh:
            stats["missing"] += 1
            buf.append((appid, None, None, "missing", checked_at))
            print(f"[{index}/{len(targets)}] {appid} -> 官方无此条目")
            if len(buf) >= 20:
                flush()
            continue

        # 分支 3：中文位返回的是纯英文 → 官方没做本地化，
        # 这个值本身就是官方英文名，不用再发第二次请求（省掉约 58% 的请求量）
        if not has_cjk(name_zh):
            stats["en_only"] += 1
            buf.append((appid, name_zh, name_zh, "en_only", checked_at))
            print(f"[{index}/{len(targets)}] {appid} -> 无中文，英文 {name_zh!r}")
            if len(buf) >= 20:
                flush()
            continue

        # 分支 4：拿到了官方中文，再取一次英文名配对
        found_en, name_en, fatal_en = fetcher.get(appid, "english")
        stats["ok"] += 1
        if fatal_en or not found_en:
            buf.append((appid, name_zh, None, "ok", checked_at))
            print(f"[{index}/{len(targets)}] {appid} -> 中文「{name_zh}」(英文名未取到)")
        else:
            buf.append((appid, name_zh, name_en, "ok", checked_at))
            print(f"[{index}/{len(targets)}] {appid} -> 中文「{name_zh}」/ {name_en!r}")
        if len(buf) >= 20:
            flush()

        if index % 50 == 0:
            done_n = stats["ok"] + stats["en_only"] + stats["missing"]
            rate = (stats["ok"] / done_n * 100) if done_n else 0.0
            elapsed = (time.time() - started) / 60
            print(
                f"    -- 进度 {index}/{len(targets)} | 官方中文命中率 {rate:.1f}% "
                f"| ok={stats['ok']} en_only={stats['en_only']} "
                f"missing={stats['missing']} error={stats['error']} "
                f"| 用时 {elapsed:.1f} 分钟"
            )

    flush()
    conn.close()
    total_ok = stats["ok"] + stats["en_only"] + stats["missing"]
    print(
        f"完成：处理 {processed} 个 | 有官方中文 {stats['ok']} "
        f"（{stats['ok'] / total_ok * 100 if total_ok else 0:.1f}%）"
        f" | 仅英文 {stats['en_only']} | 无条目 {stats['missing']} | 网络失败 {stats['error']}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
