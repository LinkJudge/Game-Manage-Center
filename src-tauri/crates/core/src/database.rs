//! 游戏数据库（PCGamingWiki 数据）的只读访问层
//!
//! schema 与原 GSM 保持一致：games(title PK, wiki_page_id UNIQUE, install_folder,
//! steam_id, gog_id, save_location JSON, platform JSON, zh_CN) + metadata。
//! steam_id/gog_id 在库里是动态类型（多为 INTEGER，gog 偶有带注释的 TEXT），统一转文本。

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
    pub wiki_page_id: i64,
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
    let (title, wiki_page_id, install_folder, steam_id, gog_id, platform, zh_cn, save_location) = row;
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
        wiki_page_id,
        install_folder,
        steam_id,
        gog_id,
        platforms,
        zh_cn,
        paths,
    }
}

const SELECT_COLUMNS: &str = "SELECT title, wiki_page_id, install_folder, steam_id, gog_id, platform, zh_CN, save_location FROM games";

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
                    " WHERE title LIKE '%{escaped}%' ESCAPE '\\' OR zh_CN LIKE '%{escaped}%' ESCAPE '\\'"
                )
            }
        };
        let sql = format!(
            "{SELECT_COLUMNS}{search_clause} ORDER BY title COLLATE NOCASE LIMIT ?1 OFFSET ?2"
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
        let sql = format!("{SELECT_COLUMNS} WHERE install_folder IN ({placeholders})");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(folders.iter()), map_row)
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
            .map(|rows| rows.into_iter().map(game_entry_from_row).collect())
    }

    /// 按游戏标题匹配（EA/R星等 install_folder 缺失时的兜底，目录名≈标题）
    pub fn list_games_by_titles(&self, titles: &[&str]) -> Result<Vec<GameEntry>, String> {
        if titles.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = titles.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("{SELECT_COLUMNS} WHERE title IN ({placeholders}) COLLATE NOCASE");
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
        let sql = format!("{SELECT_COLUMNS} ORDER BY title COLLATE NOCASE");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], map_row).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
            .map(|rows| rows.into_iter().map(game_entry_from_row).collect())
    }

    /// 单游戏条目
    pub fn get_game(&self, wiki_page_id: i64) -> Result<Option<GameEntry>, String> {
        let sql = format!("{SELECT_COLUMNS} WHERE wiki_page_id = ?1");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        let mut rows = stmt.query([wiki_page_id]).map_err(|e| e.to_string())?;
        match rows.next().map_err(|e| e.to_string())? {
            None => Ok(None),
            Some(row) => Ok(Some(game_entry_from_row(map_row(row).map_err(|e| e.to_string())?))),
        }
    }

    /// 单游戏存档路径模板（含完整 save_location JSON）
    pub fn get_game_save_locations(
        &self,
        wiki_page_id: i64,
    ) -> Result<Option<SaveLocations>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT save_location FROM games WHERE wiki_page_id = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([wiki_page_id]).map_err(|e| e.to_string())?;
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
        let page = db.list_games(Some("赛博朋克"), 10, 0).unwrap();
        assert!(!page.is_empty());
        assert!(page.iter().any(|g| g.wiki_page_id == 97605));
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
        assert!(game.zh_cn.is_some());
    }
}
