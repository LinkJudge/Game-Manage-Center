//! 路径变量解析（行为规格见 SPEC.md §1）
//!
//! 设计说明：每个变量在 `resolve_one` 中显式计算，不做静态映射表；
//! 复合变量（如 Documents）永远把后缀拼接到基底之后。

use crate::platform::Platform;
use std::path::Path;

/// 规格书 §1 中列出的全部变量名（供 UI 与调用方枚举）
pub const KNOWN: &[&str] = &[
    "username",
    "userprofile",
    "userprofile/documents",
    "userprofile/appdata/locallow",
    "appdata",
    "localappdata",
    "programfiles",
    "programdata",
    "public",
    "windir",
    "hkcu",
    "hklm",
    "wow64",
    "osxhome",
    "linuxhome",
    "xdgdatahome",
    "xdgconfighome",
    // 上下文变量：game / steam / uid / uplay / ubisoftconnect 由调用方注入
];

pub fn is_known(name: &str) -> bool {
    KNOWN.contains(&name) || matches!(name, "game" | "steam" | "uid" | "uplay" | "ubisoftconnect")
}

/// 归一化：小写、反斜杠转正斜杠
pub fn normalize(name: &str) -> String {
    name.to_lowercase().replace('\\', "/")
}

fn join(base: &str, parts: &[&str], sep: char) -> String {
    let mut out = base.trim_end_matches(['\\', '/']).to_string();
    for p in parts {
        out.push(sep);
        out.push_str(p);
    }
    out
}

/// 解析单个变量。返回 None = 不可解析（上下文变量、无来源）。
pub fn resolve_one(
    name: &str,
    platform: Platform,
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
) -> Option<String> {
    let sep = platform.separator();
    let home_s = home.to_string_lossy().to_string();

    match name {
        "username" => env("USERNAME").or_else(|| env("USER")),
        "userprofile" => Some(env("USERPROFILE").unwrap_or_else(|| home_s.clone())),
        "userprofile/documents" => Some(join(
            &env("USERPROFILE").unwrap_or_else(|| home_s.clone()),
            &["Documents"],
            sep,
        )),
        "userprofile/appdata/locallow" => Some(join(
            &env("USERPROFILE").unwrap_or_else(|| home_s.clone()),
            &["AppData", "LocalLow"],
            sep,
        )),
        "appdata" => {
            Some(env("APPDATA").unwrap_or_else(|| join(&home_s, &["AppData", "Roaming"], sep)))
        }
        "localappdata" => Some(
            env("LOCALAPPDATA").unwrap_or_else(|| join(&home_s, &["AppData", "Local"], sep)),
        ),
        "programfiles" => {
            Some(env("PROGRAMFILES").unwrap_or_else(|| "C:\\Program Files".to_string()))
        }
        "programdata" => Some(env("PROGRAMDATA").unwrap_or_else(|| "C:\\ProgramData".to_string())),
        "public" => Some(env("PUBLIC").unwrap_or_else(|| "C:\\Users\\Public".to_string())),
        "windir" => Some(env("WINDIR").unwrap_or_else(|| "C:\\Windows".to_string())),
        "hkcu" => Some("HKEY_CURRENT_USER".to_string()),
        "hklm" => Some("HKEY_LOCAL_MACHINE".to_string()),
        "wow64" => Some("HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node".to_string()),
        "osxhome" | "linuxhome" => Some(home_s),
        "xdgdatahome" => {
            Some(env("XDG_DATA_HOME").unwrap_or_else(|| join(&home_s, &[".local", "share"], sep)))
        }
        "xdgconfighome" => {
            Some(env("XDG_CONFIG_HOME").unwrap_or_else(|| join(&home_s, &[".config"], sep)))
        }
        // 上下文变量由调用方注入（SPEC §1）
        "game" | "steam" | "uid" | "uplay" | "ubisoftconnect" => None,
        _ => None,
    }
}

/// 便捷入口：当前平台 + 真实环境
pub fn resolve_now(name: &str) -> Option<String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    resolve_one(name, Platform::current(), &|k| std::env::var(k).ok(), &home)
}

/// 变量是否为注册表根键类（决定条目归入注册表桶）
pub fn is_registry_var(name: &str) -> bool {
    matches!(normalize(name).as_str(), "hkcu" | "hklm" | "wow64")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const HOME: &str = "/home/t";

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn compound_vars_always_append_suffix() {
        // SPEC §1：复合变量永远拼接后缀（环境变量只是基底）
        let env = |k: &str| {
            if k == "USERPROFILE" {
                Some("C:\\Users\\j".to_string())
            } else {
                None
            }
        };
        assert_eq!(
            resolve_one("userprofile/documents", Platform::Windows, &env, Path::new(HOME)).unwrap(),
            "C:\\Users\\j\\Documents"
        );
        assert_eq!(
            resolve_one(
                "userprofile/appdata/locallow",
                Platform::Windows,
                &env,
                Path::new(HOME)
            )
            .unwrap(),
            "C:\\Users\\j\\AppData\\LocalLow"
        );
    }

    #[test]
    fn env_wins_over_home_fallback() {
        let env = |k: &str| {
            if k == "APPDATA" {
                Some("X:\\roam".to_string())
            } else {
                None
            }
        };
        assert_eq!(
            resolve_one("appdata", Platform::Windows, &env, Path::new(HOME)).unwrap(),
            "X:\\roam"
        );
        assert_eq!(
            resolve_one("appdata", Platform::Windows, &none, Path::new(HOME)).unwrap(),
            "/home/t\\AppData\\Roaming"
        );
    }

    #[test]
    fn registry_and_fixed_vars() {
        assert_eq!(
            resolve_one("wow64", Platform::Windows, &none, Path::new(HOME)).unwrap(),
            "HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node"
        );
    }

    #[test]
    fn context_vars_are_caller_responsibility() {
        assert!(resolve_one("game", Platform::Windows, &none, Path::new(HOME)).is_none());
        assert!(resolve_one("steam", Platform::MacOS, &none, Path::new(HOME)).is_none());
    }

    #[test]
    fn known_list_covers_all_non_context() {
        for name in KNOWN {
            assert!(is_known(name));
        }
        assert!(is_known("uid") && is_known("game") && is_known("steam"));
        assert!(!is_known("madeup"));
    }

    #[test]
    fn normalize_unifies_case_and_separators() {
        assert_eq!(normalize("UserProfile\\Documents"), "userprofile/documents");
    }
}
