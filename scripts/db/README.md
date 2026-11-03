# scripts/db — 游戏库结构与迁移

游戏库 `src-tauri/resources/database.db` 的表结构由本项目定义（`gm.catalog/1`），
**不使用上游项目的 schema**。

## 表结构

```sql
games (
    id          INTEGER PRIMARY KEY,   -- 自增主键，标题不参与主键
    pcgw_id     INTEGER UNIQUE,        -- PCGamingWiki 页面 ID（稳定外部标识）
    name_en     TEXT NOT NULL,
    name_zh     TEXT,
    install_dir TEXT,
    steam_appid INTEGER,
    gog_appid   INTEGER,
    save_paths  TEXT NOT NULL DEFAULT '{}',   -- {win,reg,mac,linux,+*_kinds}
    stores      TEXT NOT NULL DEFAULT '[]'
)
+ 索引 idx_games_name_en / idx_games_name_zh / idx_games_steam / idx_games_gog

catalog_meta(key, value)                       -- 同步时间戳与 schema 版本
name_sources(pcgw_id, source, name, status, checked_at)  -- 名字来源留痕
```

**为什么不用上游结构**：上游拿 `title` 当主键，导致改名撞 PRIMARY KEY、
备份目录名随标题漂移、同系列作品被并成一条（CS:GO 与 CS2 的官方英文名都是
`Counter-Strike 2`）。改成自增 `id` 后这些问题都不存在，且名称列可加索引。

## 数据来源

| 内容 | 来源 | 采集方 |
|---|---|---|
| 存档路径、`pcgw_id`、商店 ID、平台 | PCGamingWiki（CC BY-NC-SA） | 本项目管道 |
| 英文名 / 中文名 | Steam 官方 API、小黑盒 | 本项目脚本（`scripts/zh-names/`） |
| ~~人工整理中文名~~ | ~~上游作者~~ | **不使用** |

## 更新流程

PCGamingWiki增量同步工具位于本仓库的`scripts/pcgw/`，中间结果由`apply_pcgw_delta.py`合并。合并前备份目标库，先使用`--dry-run`审阅变更；覆盖安装目录、平台与路径须显式选择对应参数。
`migrate_schema.py`仅用于导入历史结构，不需要旧fork目录。完整响应和本机维护状态不进入发布数据库。

## 中文名补采

见 `scripts/zh-names/`：

| 脚本 | 作用 |
|---|---|
| `steam_names.py` | Steam 官方中英文名，`status` 区分 `ok`（有官方中文）/`en_only` |
| `rebuild_names.py` | 按 **`小黑盒 > 官方 > 人工兜底`** 写回 `name_zh` |

小黑盒采集、库外补采及本机调度工具由维护者在本地保存，不随公开源码分发。

中文名优先级是 **小黑盒 > 官方**。Steam 官方名常带英文括注与
书名号（「斗技场的阿利娜 Alina of the Arena」「《神力科莎EVO》 Assetto Corsa EVO」），
小黑盒是国内玩家习惯的简洁叫法。

## 迁移历史

**2026-10-29**：上游 schema → `gm.catalog/1`
- 由 `title` 主键改为 `id` 自增主键
- `install_dir` 全部按本项目规则重算（5,249 行与旧值不同，不沿用上游推断值）
- `game_name_source` → `name_sources`，`metadata` → `catalog_meta`
- 中文名只保留官方与小黑盒来源，上游人工整理的 5,771 条已清空
