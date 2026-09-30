//! 平台枚举与能力判定 —— 多平台的第一等公民概念
//!
//! 数据库（PCGamingWiki 库）的 save_location 以 win/mac/linux 三键组织，
//! 这里的枚举与之一一对应（见 [`Platform::db_key`]）。

/// 应用运行的目标平台。编译期由 cfg 确定（[`Platform::current`]）。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    #[default]
    Windows,
    MacOS,
    Linux,
}

impl Platform {
    /// 全部支持的桌面平台
    pub const ALL: [Platform; 3] = [Platform::Windows, Platform::MacOS, Platform::Linux];

    /// 当前编译目标平台
    pub const fn current() -> Platform {
        #[cfg(target_os = "windows")]
        {
            Platform::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Platform::MacOS
        }
        #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
        {
            Platform::Linux
        }
    }

    /// 数据库 save_location 的键名（对应原作者库的 osKeyMap：win32→win, darwin→mac, linux→linux）
    pub fn db_key(self) -> &'static str {
        match self {
            Platform::Windows => "win",
            Platform::MacOS => "mac",
            Platform::Linux => "linux",
        }
    }

    /// 从数据库键名反解平台
    pub fn from_db_key(key: &str) -> Option<Platform> {
        match key {
            "win" => Some(Platform::Windows),
            "mac" => Some(Platform::MacOS),
            "linux" => Some(Platform::Linux),
            _ => None,
        }
    }

    /// 该平台是否有注册表（决定 save_location 的 reg 数组是否参与解析）
    pub fn supports_registry(self) -> bool {
        matches!(self, Platform::Windows)
    }

    /// 该平台路径分隔符
    pub fn separator(self) -> char {
        match self {
            Platform::Windows => '\\',
            _ => '/',
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_key_roundtrip() {
        for p in Platform::ALL {
            assert_eq!(Platform::from_db_key(p.db_key()), Some(p));
        }
        assert_eq!(Platform::from_db_key("darwin"), None);
    }

    #[test]
    fn registry_is_windows_only() {
        assert!(Platform::Windows.supports_registry());
        assert!(!Platform::MacOS.supports_registry());
        assert!(!Platform::Linux.supports_registry());
    }

    #[test]
    fn separators_match_platform_convention() {
        assert_eq!(Platform::Windows.separator(), '\\');
        assert_eq!(Platform::MacOS.separator(), '/');
        assert_eq!(Platform::Linux.separator(), '/');
    }

    #[test]
    fn current_resolves_to_a_valid_platform() {
        assert!(Platform::ALL.contains(&Platform::current()));
    }
}
