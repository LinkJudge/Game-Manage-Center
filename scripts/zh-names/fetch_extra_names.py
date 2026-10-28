#!/usr/bin/env python3
"""抓「热销榜里、但我们游戏库里没有」的那些游戏的中文名。

这些 appid 在 `games` 表里没有对应行（PCGamingWiki 未收录，或是 DLC / 工具类），
所以名字不能写进 `game_name_source`（那里靠 wiki_page_id 关联）。单独存一张表
`game_name_extra`，为将来「给热门游戏补存档路径」做准备。

用法：
  python scripts/zh-names/fetch_extra_names.py --dry-run --limit 5    # 先看范围
  python scripts/zh-names/fetch_extra_names.py --budget 500           # 跑一批
  python scripts/zh-names/fetch_extra_names.py --limit 0 --budget 5000 # 全量

节奏与 xiaoheihe_zh.py 完全一致（随机间隔 + 长停顿 + 首个 403 熔断），
共用同一个冷却标记文件，被封时两个脚本会一起停。
"""

from __future__ import annotations

import argparse
import json
import random
import sqlite3
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import xiaoheihe_zh as core  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DB = ROOT / "src-tauri" / "resources" / "database.db"
DEFAULT_HOT = ROOT.parent / ".workbuddy" / "steam-topsellers.json"

SCHEMA = """
CREATE TABLE IF NOT EXISTS game_name_extra (
    steam_appid TEXT PRIMARY KEY,
    name        TEXT,
    status      TEXT NOT NULL,      -- named / latin / no_name
    hot_rank    INTEGER,
    checked_at  TEXT NOT NULL
)
"""


def main() -> int:
    parser = argparse.ArgumentParser(description="抓库里没有的热门游戏中文名")
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--hot", type=Path, default=DEFAULT_HOT)
    parser.add_argument("--limit", type=int, default=0, help="最多处理多少条（0=全部）")
    parser.add_argument("--budget", type=int, default=500, help="本轮最多发多少请求")
    parser.add_argument("--min-delay", type=float, default=3.0)
    parser.add_argument("--max-delay", type=float, default=6.0)
    parser.add_argument("--pause-every", type=int, nargs=2, default=[40, 60])
    parser.add_argument("--pause-range", type=float, nargs=2, default=[45, 90])
    parser.add_argument("--cooldown-hours", type=float, default=6.0)
    parser.add_argument("--force", action="store_true")
    parser.add_argument("--use-proxy", action="store_true",
                        help="走系统代理（默认直连；小黑盒是国内站，直连更安全）")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    if not args.db.exists():
        print(f"[错误] 找不到数据库：{args.db}")
        return 1
    if not args.hot.exists():
        print(f"[错误] 找不到热销榜：{args.hot}")
        return 1

    blocked_at = core.read_block_marker()
    if blocked_at and not args.force:
        from datetime import timedelta

        wait_until = blocked_at + timedelta(hours=args.cooldown_hours)
        if datetime.now() < wait_until:
            print(f"[中止] 上次触发风控于 {blocked_at:%Y-%m-%d %H:%M}，冷却到 {wait_until:%Y-%m-%d %H:%M}")
            return 3

    hot = json.loads(args.hot.read_text(encoding="utf-8"))
    rank = {str(item["appid"]): int(item["rank"]) for item in hot}

    conn = sqlite3.connect(args.db)
    conn.execute(SCHEMA)
    conn.commit()

    db_appids: set[str] = set()
    for (sid,) in conn.execute(
        "SELECT steam_id FROM games WHERE steam_id IS NOT NULL AND trim(CAST(steam_id AS TEXT))<>''"
    ):
        appid = core.steam_appid(sid)
        if appid:
            db_appids.add(appid)

    done: set[str] = {
        str(r[0]) for r in conn.execute("SELECT steam_appid FROM game_name_extra")
    }

    targets = [
        appid
        for appid in rank
        if appid not in db_appids and appid not in done
    ]
    targets.sort(key=lambda a: rank[a])
    if args.limit > 0:
        targets = targets[: args.limit]

    print(f"热销榜 {len(rank)} 个；库内 {len(db_appids & set(rank))} 个；"
          f"库外 {len(set(rank) - db_appids)} 个；已抓过 {len(done)} 个")
    print(f"本轮待处理：{len(targets)} 个，请求预算 {args.budget}")
    if not targets:
        conn.close()
        print("没有待处理记录。")
        return 0
    if args.dry_run:
        print("[dry-run] 前 10 个：")
        for appid in targets[:10]:
            print(f"  #{rank[appid]:>5}  {appid}")
        conn.close()
        return 0

    session = core.Session(
        min_delay=args.min_delay,
        max_delay=args.max_delay,
        pause_every=tuple(args.pause_every),
        pause_range=tuple(args.pause_range),
        use_proxy=args.use_proxy,
    )
    print("正在模拟浏览器访问站点（热身）……")
    try:
        session.warm_up()
    except core.Blocked as blocked:
        core.write_block_marker(str(blocked))
        print(f"[中止] 热身被拦：{blocked}")
        conn.close()
        return 2

    named = latin = missing = failed = 0
    requests_made = 0
    exit_code = 0
    buf: list[tuple[str, str | None, str, int, str]] = []

    def flush() -> None:
        if not buf:
            return
        conn.executemany(
            """
            INSERT INTO game_name_extra(steam_appid, name, status, hot_rank, checked_at)
            VALUES(?, ?, ?, ?, ?)
            ON CONFLICT(steam_appid) DO UPDATE SET
                name = excluded.name,
                status = excluded.status,
                hot_rank = excluded.hot_rank,
                checked_at = excluded.checked_at
            """,
            buf,
        )
        conn.commit()
        buf.clear()

    for index, appid in enumerate(targets, 1):
        if requests_made >= args.budget:
            print(f"已达请求预算 {args.budget}，收工。重跑会从剩余记录继续。")
            break
        session.breathe()
        requests_made += 1
        try:
            name = session.fetch_name(appid)
        except core.Blocked as blocked:
            core.write_block_marker(str(blocked))
            print(f"[{index}/{len(targets)}] {appid} -> 触发风控：{blocked}，立即中止")
            exit_code = 2
            break
        except Exception as error:  # noqa: BLE001 - 单条失败只跳过
            failed += 1
            print(f"[{index}/{len(targets)}] {appid} -> 失败：{error}")
            continue

        checked_at = datetime.now().isoformat(timespec="seconds")
        if not name:
            missing += 1
            buf.append((appid, None, "no_name", rank[appid], checked_at))
            print(f"[{index}/{len(targets)}] {appid} -> 无名字")
        elif core.has_chinese(name):
            named += 1
            buf.append((appid, name, "named", rank[appid], checked_at))
            print(f"[{index}/{len(targets)}] {appid} -> {name}")
        else:
            latin += 1
            buf.append((appid, name, "latin", rank[appid], checked_at))
            print(f"[{index}/{len(targets)}] {appid} -> 非中文「{name}」")

        if len(buf) >= 20:
            flush()

    flush()
    conn.close()
    print(
        f"完成：请求 {requests_made} 次，中文名 {named}，非中文名 {latin}，"
        f"无名字 {missing}，失败 {failed}"
    )
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
