#!/usr/bin/env python3
"""把游戏库迁移到本项目自建的表结构。

背景：旧库沿用上游 Game Save Manager 的 schema（`title` 作主键），
带来三个实际问题——改名会撞 PRIMARY KEY、备份目录名随标题漂移、
不同作品可能被合并（CS:GO 与 CS2 都叫 "Counter-Strike 2"）。

新结构改用自增 `id` 作主键，`pcgw_id` 作为稳定外部标识，
关键字加索引，使搜索走索引而非全表扫描。

字段映射：
    title          -> name_en
    wiki_page_id   -> pcgw_id
    install_folder -> install_dir   （**重新计算**，不沿用旧值）
    steam_id       -> steam_appid
    gog_id         -> gog_appid
    save_location  -> save_paths
    platform       -> stores
    zh_CN          -> name_zh
    metadata       -> catalog_meta  (key/value)

用法：
    python scripts/db/migrate_schema.py            # 迁移并生成新库
    python scripts/db/migrate_schema.py --dry-run  # 只报告不写
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import sqlite3
import sys
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SRC_DB = ROOT / "src-tauri" / "resources" / "database.db"
BACKUP_DIR = ROOT.parent / ".workbuddy" / "database-backups"

SCHEMA = """
CREATE TABLE games (
    id          INTEGER PRIMARY KEY,
    pcgw_id     INTEGER UNIQUE,
    name_en     TEXT NOT NULL,
    name_zh     TEXT,
    install_dir TEXT,
    steam_appid INTEGER,
    gog_appid   INTEGER,
    save_paths  TEXT NOT NULL DEFAULT '{}',
    stores      TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX idx_games_name_en ON games(name_en);
CREATE INDEX idx_games_name_zh ON games(name_zh);
CREATE INDEX idx_games_steam   ON games(steam_appid);
CREATE INDEX idx_games_gog     ON games(gog_appid);

CREATE TABLE catalog_meta (
    key   TEXT PRIMARY KEY,
    value TEXT
);

-- 名字来源留痕（官方 / 小黑盒），便于追溯与增量补采
CREATE TABLE name_sources (
    pcgw_id    INTEGER NOT NULL,
    source     TEXT NOT NULL,
    name       TEXT,
    status     TEXT NOT NULL,
    checked_at TEXT NOT NULL,
    PRIMARY KEY (pcgw_id, source)
);

CREATE INDEX idx_name_sources_status ON name_sources(status);

-- Steam 官方中英文名（按 Steam AppID，独立于 PCGW 收录情况）
CREATE TABLE IF NOT EXISTS steam_names (
    steam_appid TEXT PRIMARY KEY,
    name_zh     TEXT,
    name_en     TEXT,
    status      TEXT NOT NULL,
    checked_at  TEXT NOT NULL
);

-- 库外游戏名字（Steam 热销榜里 PCGW 未收录的，没有 pcgw_id）
CREATE TABLE IF NOT EXISTS extra_names (
    steam_appid TEXT PRIMARY KEY,
    name        TEXT,
    status      TEXT NOT NULL,
    hot_rank    INTEGER,
    checked_at  TEXT NOT NULL
);
"""

INVALID = re.compile(r'[\\/:*?"<>|]')


def make_install_dir(name_en: str | None) -> str | None:
    """按本项目规则推导安装目录名：去 Windows 非法字符、压缩空白、去首尾点和空格。"""
    if not name_en:
        return None
    s = INVALID.sub("", name_en)
    s = re.sub(r"\s+", " ", s).strip(" .")
    return s or None


def as_int(v):
    if v is None:
        return None
    if isinstance(v, int):
        return v
    s = str(v).strip()
    return int(s) if s.isdigit() else None


def main() -> int:
    parser = argparse.ArgumentParser(description="迁移游戏库到自建 schema")
    parser.add_argument("--db", type=Path, default=SRC_DB, help="输入库（旧结构或新结构均可）")
    parser.add_argument("--out", type=Path, default=None, help="输出库路径（默认 <db>.new.db）")
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    if not args.db.exists():
        print(f"[错误] 找不到数据库：{args.db}")
        return 1

    src = sqlite3.connect(args.db)
    old_cols = {r[1] for r in src.execute("PRAGMA table_info(games)")}
    if "wiki_page_id" not in old_cols:
        print("[跳过] 已经是新结构，无需迁移。")
        return 0

    rows = src.execute(
        "SELECT title, wiki_page_id, install_folder, steam_id, gog_id, "
        "save_location, platform, zh_CN FROM games"
    ).fetchall()
    print(f"读取旧库 {len(rows)} 行")

    # 旧 metadata → 新 catalog_meta
    meta = {}
    try:
        for k, v in src.execute("SELECT * FROM metadata"):
            meta[k] = v
    except sqlite3.Error:
        pass
    meta["schema"] = "gm.catalog/1"
    meta["migrated_at"] = datetime.now().isoformat(timespec="seconds")

    # 三张名字来源表（若存在则一并搬迁，避免增量补采丢失进度）
    tables = {r[0] for r in src.execute("SELECT name FROM sqlite_master WHERE type='table'")}

    sources = []
    if "game_name_source" in tables:
        sources = src.execute(
            "SELECT wiki_page_id, source, name, status, checked_at FROM game_name_source"
        ).fetchall()
        print(f"读取名字来源 {len(sources)} 条")

    official = []
    if "game_name_official" in tables:
        official = src.execute(
            "SELECT steam_appid, name_zh, name_en, status, checked_at FROM game_name_official"
        ).fetchall()
        print(f"读取 Steam 官方名 {len(official)} 条")

    extra = []
    if "game_name_extra" in tables:
        extra = src.execute(
            "SELECT steam_appid, name, status, hot_rank, checked_at FROM game_name_extra"
        ).fetchall()
        print(f"读取库外游戏名 {len(extra)} 条")

    if args.dry_run:
        changed = sum(1 for r in rows if make_install_dir(r[0]) != (r[2] or None))
        print(f"[dry-run] 将写入 {len(rows)} 行；install_dir 与旧值不同 {changed} 行")
        src.close()
        return 0

    BACKUP_DIR.mkdir(parents=True, exist_ok=True)
    stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    backup = BACKUP_DIR / f"database-before-schema-migration-{stamp}.db"
    src.close()
    shutil.copy2(args.db, backup)
    print(f"已备份旧库：{backup}")

    new_path = args.out if args.out else args.db.with_suffix(".new.db")
    if new_path.exists():
        new_path.unlink()
    dst = sqlite3.connect(new_path)
    dst.executescript(SCHEMA)

    game_rows = []
    for title, pcgw_id, _old_folder, steam_id, gog_id, save_location, platform, zh_cn in rows:
        game_rows.append((
            pcgw_id,
            title,
            (zh_cn or "").strip() or None,
            make_install_dir(title),
            as_int(steam_id),
            as_int(gog_id),
            save_location or "{}",
            platform or "[]",
        ))
    dst.executemany(
        "INSERT INTO games(pcgw_id, name_en, name_zh, install_dir, steam_appid, "
        "gog_appid, save_paths, stores) VALUES(?,?,?,?,?,?,?,?)",
        game_rows,
    )
    dst.executemany(
        "INSERT INTO catalog_meta(key, value) VALUES(?,?)", list(meta.items())
    )
    if sources:
        dst.executemany(
            "INSERT OR REPLACE INTO name_sources(pcgw_id, source, name, status, checked_at) "
            "VALUES(?,?,?,?,?)",
            sources,
        )
    if official:
        dst.executemany(
            "INSERT OR REPLACE INTO steam_names(steam_appid, name_zh, name_en, status, checked_at) "
            "VALUES(?,?,?,?,?)",
            official,
        )
    if extra:
        dst.executemany(
            "INSERT OR REPLACE INTO extra_names(steam_appid, name, status, hot_rank, checked_at) "
            "VALUES(?,?,?,?,?)",
            extra,
        )
    dst.commit()

    # 校验
    print("\n=== 校验 ===")
    n_new = dst.execute("SELECT COUNT(*) FROM games").fetchone()[0]
    n_zh = dst.execute(
        "SELECT COUNT(*) FROM games WHERE name_zh IS NOT NULL AND trim(name_zh)<>''"
    ).fetchone()[0]
    n_paths = dst.execute(
        "SELECT COUNT(*) FROM games WHERE save_paths IS NOT NULL AND trim(save_paths)<>'' AND save_paths<>'{}'"
    ).fetchone()[0]
    n_src = dst.execute("SELECT COUNT(*) FROM name_sources").fetchone()[0]
    n_off = dst.execute("SELECT COUNT(*) FROM steam_names").fetchone()[0]
    n_ext = dst.execute("SELECT COUNT(*) FROM extra_names").fetchone()[0]
    print(f"  游戏行数      {n_new}")
    print(f"  有中文名      {n_zh}")
    print(f"  有存档路径    {n_paths}")
    print(f"  名字来源记录  {n_src}")
    print(f"  Steam 官方名  {n_off}")
    print(f"  库外游戏名    {n_ext}")
    print(f"  integrity     {dst.execute('PRAGMA integrity_check').fetchone()[0]}")

    dup = dst.execute(
        "SELECT name_en, COUNT(*) c FROM games GROUP BY name_en HAVING c>1 LIMIT 5"
    ).fetchall()
    print(f"  重名（现在允许，因主键是 id）: {len(dup)} 组" + (f"，例：{dup[:3]}" if dup else ""))

    dst.close()
    print(f"\n新库已生成：{new_path}")
    print("确认无误后替换：")
    print(f'  move "{new_path.name}" "database.db"')
    return 0


if __name__ == "__main__":
    sys.exit(main())
