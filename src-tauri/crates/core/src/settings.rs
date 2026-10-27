//! 应用设置持久化（settings.json，存于应用数据目录）
//!
//! 键位按新应用语义定义；autoBackupEnabled 为全局自动备份开关——
//! 开启后所有已安装游戏"存档变化即备份 + 游戏退出时备份"。

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// 备份根目录；None = 默认（应用数据目录/Backups）
    pub backup_path: Option<String>,
    /// 每游戏滚动备份上限（永久固定的不计入）
    pub max_backups: u32,
    /// 备份压缩为 .zip（默认不压缩，需手动开启）
    pub compress_backups: bool,
    /// 全局自动备份开关：开启后所有已安装游戏"存档变化即备份 + 游戏退出时备份"
    pub auto_backup_enabled: bool,
    /// 关闭主窗口后进入托盘（true，默认）还是直接退出（false）
    pub close_to_tray: bool,
    /// 修改器安装根目录；None = 默认（应用数据目录/Trainers）
    pub trainer_path: Option<String>,
    /// SteamGridDB API Key（仅保存在本机 settings.json，用于手动选图与补全封面）
    pub steamgriddb_api_key: Option<String>,
    /// 界面语言：None = 跟随系统；"zh" 中文；"en" English
    pub locale: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            backup_path: None,
            max_backups: 5,
            // 默认不压缩——压缩必须由用户在设置里手动开启
            compress_backups: false,
            auto_backup_enabled: false,
            // 默认关窗进托盘（自动备份常驻）
            close_to_tray: true,
            trainer_path: None,
            steamgriddb_api_key: None,
            locale: None,
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// 临时文件 + 改名写入，避免损坏
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_for_missing_file() {
        let path = std::env::temp_dir().join(format!("gm-settings-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let settings = AppSettings::load(&path);
        assert_eq!(settings.max_backups, 5);
        assert!(settings.backup_path.is_none());
        assert!(!settings.compress_backups, "默认不压缩");
        assert!(!settings.auto_backup_enabled, "默认不开启自动备份");
        assert!(settings.close_to_tray, "默认关窗进托盘");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_compress_key_loads_as_uncompressed() {
        // 早期 settings.json 没有 compressBackups 键 → 必须按"不压缩"处理
        let path = std::env::temp_dir().join(format!("gm-settings-old-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"backupPath":null,"maxBackups":5,"autoBackupGames":{}}"#,
        )
        .unwrap();
        let settings = AppSettings::load(&path);
        assert!(!settings.compress_backups, "缺失键按默认不压缩");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn roundtrip_preserves_settings() {
        let path = std::env::temp_dir().join(format!("gm-settings-rt-{}.json", std::process::id()));
        let settings = AppSettings {
            max_backups: 8,
            backup_path: Some("D:\\MyBackups".into()),
            compress_backups: true,
            auto_backup_enabled: true,
            close_to_tray: false,
            trainer_path: Some("D:\\MyTrainers".into()),
            steamgriddb_api_key: Some("sgdb-test".into()),
        };
        settings.save(&path).unwrap();

        let loaded = AppSettings::load(&path);
        assert_eq!(loaded.max_backups, 8);
        assert_eq!(loaded.backup_path.as_deref(), Some("D:\\MyBackups"));
        assert!(loaded.compress_backups);
        assert!(loaded.auto_backup_enabled);
        assert!(!loaded.close_to_tray);
        assert_eq!(loaded.trainer_path.as_deref(), Some("D:\\MyTrainers"));
        assert_eq!(loaded.steamgriddb_api_key.as_deref(), Some("sgdb-test"));
        let _ = std::fs::remove_file(&path);
    }
}
