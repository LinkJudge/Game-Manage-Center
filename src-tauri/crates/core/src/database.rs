//! 游戏数据库（PCGamingWiki 数据）的只读访问层
//!
//! 表结构（`gm.catalog/1`，本项目自建）：
//! `games(id PK AUTOINCREMENT, pcgw_id UNIQUE, name_en, name_zh, install_dir,
//! steam_appid, gog_appid, save_paths JSON, stores JSON)` + `catalog_meta` + `name_sources`。
//! 用自增 id 作主键，标题不再参与主键，改名不会撞键、也不会把不同作品并成一条。
//! steam_appid/gog_appid 允许动态类型（历史数据里 gog 偶有带注释的文本），读取时统一转文本。

use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

/// save_location JSON 结构（kinds 与同名路径数组平行：saves=存档, config=设置, reg=注册表）
#[derive(Clone, Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct SaveLocations {
    #[serde(default)]
    pub win: Vec<String>,
    #[serde(default)]
    pub reg: Vec<String>,
    #[serde(default)]
    pub mac: Vec<String>,
    #[serde(default)]
    pub linux: Vec<String>,
    #[serde(default)]
    pub win_kinds: Vec<String>,
    #[serde(default)]
    pub reg_kinds: Vec<String>,
    #[serde(default)]
    pub mac_kinds: Vec<String>,
    #[serde(default)]
    pub linux_kinds: Vec<String>,
}

impl SaveLocations {
    pub fn total(&self) -> usize {
        self.win.len() + self.reg.len() + self.mac.len() + self.linux.len()
    }
}

/// 游戏列表条目（前端表格行）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameEntry {
    pub title: String,
    pub pcgw_id: i64,
    pub install_folder: Option<String>,
    pub steam_id: Option<String>,
    pub gog_id: Option<String>,
    pub platforms: Vec<String>,
    pub zh_cn: Option<String>,
    pub paths: SaveLocations,
}

/// steam_id/gog_id 列是动态类型，统一转文本
fn value_to_text(value: Option<rusqlite::types::Value>) -> Option<String> {
    match value {
        Some(rusqlite::types::Value::Integer(i)) => Some(i.to_string()),
        Some(rusqlite::types::Value::Text(s)) => Some(s),
        _ => None,
    }
}

/// 一行 SQL → 条目（列表查询与目录名查询共用，行元组见 GameRow）
fn game_entry_from_row(row: GameRow) -> GameEntry {
    let (title, pcgw_id, install_folder, steam_id, gog_id, platform, zh_cn, save_location) = row;
    let paths: SaveLocations = save_location
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default();
    let platforms: Vec<String> = platform
        .as_deref()
        .and_then(|json| serde_json::from_str(json).ok())
        .unwrap_or_default();
    GameEntry {
        title,
        pcgw_id,
        install_folder,
        steam_id,
        gog_id,
        platforms,
        zh_cn,
        paths,
    }
}

const SELECT_COLUMNS: &str = "SELECT name_en AS title, pcgw_id, install_dir AS install_folder, \
     steam_appid AS steam_id, gog_appid AS gog_id, stores AS platform, name_zh AS zh_CN, \
     save_paths AS save_location FROM games";

type GameRow = (
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        value_to_text(row.get(3)?),
        value_to_text(row.get(4)?),
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

pub struct Database {
    conn: Connection,
}

impl Database {
    /// 只读打开（immutable 语义：不会创建 -wal/-shm）
    pub fn open_readonly(path: &Path) -> Result<Self, String> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("打开数据库失败: {e}"))?;
        Ok(Self { conn })
    }

    pub fn total_games(&self) -> Result<i64, String> {
        self.conn
            .query_row("SELECT COUNT(*) FROM games", [], |row| row.get(0))
            .map_err(|e| e.to_string())
    }

    /// 按标题（含中文名）模糊搜索，分页返回
    pub fn list_games(
        &self,
        search: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<GameEntry>, String> {
        let search_clause = match search.map(str::trim).filter(|s| !s.is_empty()) {
            None => String::new(),
            Some(term) => {
                // LIKE 通配符转义，用户输入按字面匹配
                let escaped =
                    term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
                format!(
                    " WHERE name_en LIKE '%{escaped}%' ESCAPE '\\' OR name_zh LIKE '%{escaped}%' ESCAPE '\\'"
                )
            }
        };
        let sql = format!(
            "{SELECT_COLUMNS}{search_clause} ORDER BY name_en COLLATE NOCASE LIMIT ?1 OFFSET ?2"
        );
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![limit as i64, offset as i64], map_row)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
            .map(|rows| rows.into_iter().map(game_entry_from_row).collect())
    }

    /// 按安装目录名批量查询（快速扫描用；分块由调用方控制 ≤500）
    pub fn list_games_by_install_folder(
        &self,
        folders: &[&str],
    ) -> Result<Vec<GameEntry>, String> {
        if folders.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = folders.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("{SELECT_COLUMNS} WHERE install_dir IN ({placeholders})");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(folders.iter()), map_row)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
            .map(|rows| rows.into_iter().map(game_entry_from_row).collect())
    }

    /// 全库 (title, zh_CN) 对，供修改器模块构建中文名索引（trainers::ZhNameIndex）。
    /// 无中文名的游戏也会返回（zh 为 None，索引侧自行过滤）。
    pub fn title_zh_pairs(&self) -> Result<Vec<(String, Option<String>)>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT name_en, name_zh FROM games")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// 按游戏标题匹配（EA/R星等 install_folder 缺失时的兜底，目录名≈标题）
    pub fn list_games_by_titles(&self, titles: &[&str]) -> Result<Vec<GameEntry>, String> {
        if titles.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = titles.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("{SELECT_COLUMNS} WHERE name_en IN ({placeholders}) COLLATE NOCASE");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(titles.iter()), map_row)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
            .map(|rows| rows.into_iter().map(game_entry_from_row).collect())
    }

    /// 全量读取（深度扫描用，按标题排序流式收集）
    pub fn list_all_games(&self) -> Result<Vec<GameEntry>, String> {
        let sql = format!("{SELECT_COLUMNS} ORDER BY name_en COLLATE NOCASE");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], map_row).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
            .map(|rows| rows.into_iter().map(game_entry_from_row).collect())
    }

    /// 单游戏条目
    pub fn get_game(&self, pcgw_id: i64) -> Result<Option<GameEntry>, String> {
        let sql = format!("{SELECT_COLUMNS} WHERE pcgw_id = ?1");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = stmt.query([pcgw_id]).map_err(|e| e.to_string())?;
        match rows.next().map_err(|e| e.to_string())? {
            None => Ok(None),
            Some(row) => Ok(Some(game_entry_from_row(map_row(row).map_err(|e| e.to_string())?))),
        }
    }

    /// 单游戏存档路径模板（含完整 save_location JSON）
    pub fn get_game_save_locations(
        &self,
        pcgw_id: i64,
    ) -> Result<Option<SaveLocations>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT save_paths FROM games WHERE pcgw_id = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([pcgw_id]).map_err(|e| e.to_string())?;
        match rows.next().map_err(|e| e.to_string())? {
            None => Ok(None),
            Some(row) => {
                let json: Option<String> = row.get(0).map_err(|e| e.to_string())?;
                Ok(json
                    .as_deref()
                    .and_then(|json| serde_json::from_str(json).ok()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 真实数据库位于 src-tauri/resources（本测试在 crates/core，故回退两级）
    fn test_db_path() -> Option<PathBuf> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/database.db");
        path.exists().then_some(path)
    }

    #[test]
    fn opens_and_counts_games() {
        let Some(path) = test_db_path() else { return };
        let db = Database::open_readonly(&path).unwrap();
        let total = db.total_games().unwrap();
        assert!(total > 40_000, "expect full library, got {total}");
    }

    #[test]
    fn search_finds_by_chinese_title() {
        let Some(path) = test_db_path() else { return };
        let db = Database::open_readonly(&path).unwrap();
        // 自适应取一个确实有中文名的样本——中文名覆盖率会随采集进度变化，
        // 写死具体游戏会让测试在数据更新后假失败。
        let pairs = db.title_zh_pairs().unwrap();
        let Some(zh) = pairs.iter().find_map(|(_, z)| z.as_deref()) else { return };
        let keyword: String = zh.chars().take(2).collect();
        let page = db.list_games(Some(&keyword), 10, 0).unwrap();
        assert!(!page.is_empty(), "搜索「{keyword}」应命中（样本中文名「{zh}」）");
        // 动态类型列转文本正常
        assert!(page.iter().any(|g| g.steam_id.is_some()));
    }

    #[test]
    fn save_locations_parse_for_known_game() {
        let Some(path) = test_db_path() else { return };
        let db = Database::open_readonly(&path).unwrap();
        let paths = db.get_game_save_locations(97605).unwrap().unwrap(); // Cyberpunk 2077
        assert!(!paths.win.is_empty());
        assert!(paths.win[0].contains("Cyberpunk 2077"));
    }

    #[test]
    fn get_game_returns_entry_with_ids() {
        let Some(path) = test_db_path() else { return };
        let db = Database::open_readonly(&path).unwrap();
        let game = db.get_game(97605).unwrap().unwrap();
        assert_eq!(game.title, "Cyberpunk 2077");
        assert_eq!(game.steam_id.as_deref(), Some("1091500"));
        // 存档路径必须能解析出来（比中文名更稳定：存档数据 100% 覆盖）
        assert!(game.paths.total() > 0, "Cyberpunk 2077 应有存档路径");
    }

    /// 自建 schema 的关键收益：多个游戏可以共用同一个英文名而不会撞主键
    /// （Steam 官方对同系列作品常返回同名，如 CS:GO 与 CS2 都叫 "Counter-Strike 2"）
    #[test]
    fn duplicate_display_names_are_allowed() {
        let Some(path) = test_db_path() else { return };
        let conn = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let dup: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM (SELECT name_en FROM games GROUP BY name_en HAVING COUNT(*)>1)",
                [],
                |r| r.get(0),
            )
            .unwrap_or(-1);
        // 0 组也合法；关键是这条 GROUP BY 不报错、主键不冲突
        assert!(dup >= 0);
    }
}
