//! 自定义游戏 —— 手动添加任意路径 / 注册表项到备份系统
//!
//! 持久化为应用数据目录下的 custom_games.json；路径支持与数据库相同的
//! 占位符模板（{{p|appdata}} 等），跨机器可用。id 使用负数（正数留给
//! PCGamingWiki 数据库的 pcgw_id），备份目录/恢复/轮换全部复用主引擎。

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomPath {
    /// "folder" | "file" | "reg"
    pub kind: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomGame {
    /// 负数 id；0 表示待分配（保存时由调用方分配）
    pub id: i64,
    pub name: String,
    pub paths: Vec<CustomPath>,
}

pub fn load(path: &Path) -> Vec<CustomGame> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, games: &[CustomGame]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(games).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// 为 id==0 的新条目分配负数 id（现有最小 id - 1）
pub fn assign_ids(games: &mut [CustomGame]) {
    let mut next = games.iter().map(|g| g.id).min().unwrap_or(0);
    for g in games.iter_mut() {
        if g.id == 0 {
            next -= 1;
            g.id = next;
        }
    }
}

/// 转成主引擎的游戏条目：folder/file → win 桶模板，reg → reg 桶
pub fn to_game_entry(custom: &CustomGame) -> crate::database::GameEntry {
    let mut win = Vec::new();
    let mut reg = Vec::new();
    for p in &custom.paths {
        match p.kind.as_str() {
            "reg" => reg.push(p.path.clone()),
            _ => win.push(p.path.clone()),
        }
    }
    crate::database::GameEntry {
        title: custom.name.clone(),
        pcgw_id: custom.id,
        install_folder: None,
        steam_id: None,
        gog_id: None,
        platforms: vec!["自定义".into()],
        zh_cn: None,
        zh_hant: None,
        paths: crate::database::SaveLocations {
            win,
            reg,
            ..Default::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_assign_ids() {
        let path = std::env::temp_dir().join(format!("gm-custom-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut games = vec![CustomGame {
            id: 0,
            name: "My Modded Game".into(),
            paths: vec![
                CustomPath { kind: "folder".into(), path: "{{p|game}}\\mods".into() },
                CustomPath { kind: "reg".into(), path: "HKEY_CURRENT_USER\\Software\\MyGame".into() },
            ],
        }];
        assign_ids(&mut games);
        assert_eq!(games[0].id, -1);
        save(&path, &games).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "My Modded Game");
        assert_eq!(loaded[0].paths.len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn converts_to_game_entry() {
        let custom = CustomGame {
            id: -5,
            name: "X".into(),
            paths: vec![CustomPath { kind: "file".into(), path: "{{p|appdata}}\\X\\save.dat".into() }],
        };
        let entry = to_game_entry(&custom);
        assert_eq!(entry.pcgw_id, -5);
        assert_eq!(entry.paths.win.len(), 1);
        assert_eq!(entry.platforms, vec!["自定义"]);
    }
}
