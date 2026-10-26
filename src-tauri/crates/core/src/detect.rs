//! 环境探测：启动器安装目录 + 平台账号 ID
//!
//! 解析逻辑抽成纯函数（可单测），Windows 胶水层读注册表与真实文件。
//! macOS / Linux 探测是下一阶段的模块（接口已留好，cfg 门控）。

use crate::platform::Platform;
use crate::resolve::AccountIds;
use serde::Serialize;
use std::collections::HashMap;

/// 一个游戏库根目录及其所属平台（用于"游戏装在哪个平台"的判定）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRoot {
    pub path: String,
    /// Steam / Epic / Ubisoft / EA / GOG / Battle.net / Rockstar
    pub platform: String,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedEnvironment {
    pub platform: Platform,
    /// 全部游戏库根目录（按平台标记，供 install_folder 匹配与安装平台判定）
    pub install_paths: Vec<InstallRoot>,
    /// Steam appid → 实际安装目录（来自 appmanifest_*.acf 的 installdir）
    pub steam_apps: HashMap<String, String>,
    pub steam_path: Option<String>,
    pub ubisoft_path: Option<String>,
    pub accounts: AccountIds,
}

// ======================================================================
// 纯解析函数（平台无关，全部可单测）
// ======================================================================

/// Epic LauncherInstalled.dat → 各安装位置的父目录
pub fn epic_install_roots_from_json(json: &str) -> Vec<String> {
    let value: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut roots = Vec::new();
    if let Some(list) = value.get("InstallationList").and_then(|v| v.as_array()) {
        for item in list {
            if let Some(loc) = item.get("InstallLocation").and_then(|v| v.as_str()) {
                if let Some(parent) = parent_dir(loc) {
                    if !roots.contains(&parent) {
                        roots.push(parent);
                    }
                }
            }
        }
    }
    roots
}

/// GOG Galaxy config.json → libraryPath
pub fn gog_install_root_from_json(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value
        .get("libraryPath")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Battle.net.config → Client.Install.DefaultInstallPath
pub fn battlenet_install_root_from_json(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    value
        .get("Client")?
        .get("Install")?
        .get("DefaultInstallPath")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// EA user_*.ini → user.downloadinplacedir= 的值（可能多行）
pub fn ea_install_roots_from_ini(text: &str) -> Vec<String> {
    let mut roots = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("user.downloadinplacedir=") {
            let value = value.trim();
            if !value.is_empty() && !roots.contains(&value.to_string()) {
                roots.push(value.to_string());
            }
        }
    }
    roots
}

/// EA 账号在文件名里：user_<id>.ini
pub fn ea_account_from_filename(filename: &str) -> Option<String> {
    let base = filename.strip_prefix("user_")?.strip_suffix(".ini")?;
    if base.is_empty() { None } else { Some(base.to_string()) }
}

/// Ubisoft settings.yaml → misc.game_installation_path（最小解析：按行找键）
pub fn ubisoft_install_root_from_yaml(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("game_installation_path:") {
            let value = rest.trim().trim_matches('"');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// 目录条目里取"最近修改"的账号目录（Ubisoft savegames / Rockstar Profiles 同款逻辑）
/// entries: (目录名, 修改时间秒)
pub fn latest_account_from_entries(entries: &[(String, u64)]) -> Option<String> {
    let mut best: Option<(&str, u64)> = None;
    for (name, mtime) in entries {
        if *mtime >= best.map(|(_, t)| t).unwrap_or(0) {
            best = Some((name.as_str(), *mtime));
        }
    }
    // 有效性检查：Ubisoft 要求纯数字目录名
    best.map(|(name, _)| name.to_string())
        .filter(|name| !name.is_empty())
}

/// Epic 账号数据文件名：OC_<hex>.dat 或 <hex>.dat
pub fn epic_account_from_filename(filename: &str) -> Option<String> {
    let base = filename.strip_prefix("OC_").unwrap_or(filename);
    let base = base.strip_suffix(".dat")?;
    if base.is_empty() { None } else { Some(base.to_string()) }
}

fn parent_dir(path: &str) -> Option<String> {
    std::path::Path::new(path).parent().map(|p| p.to_string_lossy().to_string())
}

// ======================================================================
// 平台探测入口
// ======================================================================

pub fn detect() -> DetectedEnvironment {
    #[cfg(target_os = "windows")]
    {
        detect_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        // macOS / Linux 探测模块在后续阶段实现，接口已就位
        DetectedEnvironment {
            platform: Platform::current(),
            ..Default::default()
        }
    }
}

// ======================================================================
// Windows 实现
// ======================================================================

#[cfg(target_os = "windows")]
pub fn detect_windows() -> DetectedEnvironment {
    let mut env = DetectedEnvironment {
        platform: Platform::Windows,
        ..Default::default()
    };
    let mut install_paths: Vec<InstallRoot> = Vec::new();

    // ---- Steam ----
    let steam_path = reg_get_string(
        winreg::enums::HKEY_LOCAL_MACHINE,
        r"SOFTWARE\WOW6432Node\Valve\Steam",
        "InstallPath",
    )
    .or_else(|| existing("C:\\Program Files (x86)\\Steam"));
    if let Some(steam) = &steam_path {
        env.steam_path = Some(steam.clone());
        let vdf_path = std::path::Path::new(steam).join("config").join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&vdf_path) {
            if let Ok(vdf) = crate::vdf::parse_vdf(&text) {
                for root in crate::vdf::steam_library_roots(&vdf) {
                    // appid → 安装目录：直接枚举 steamapps 下的 appmanifest_*.acf——
                    // 磁盘上的 acf 是"已安装"的权威事实；vdf 的 apps 列表由 Steam
                    // 定期回写，可能滞后/遗漏（实测：Steep 已装但不在 apps 里）
                    let steamapps = std::path::Path::new(&root).join("steamapps");
                    if let Ok(entries) = std::fs::read_dir(&steamapps) {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            let Some(appid) = name
                                .strip_prefix("appmanifest_")
                                .and_then(|s| s.strip_suffix(".acf"))
                            else {
                                continue;
                            };
                            if let Ok(acf_text) = std::fs::read_to_string(entry.path()) {
                                if let Ok(acf) = crate::vdf::parse_vdf(&acf_text) {
                                    if let Some(installdir) = crate::vdf::acf_installdir(&acf) {
                                        let install_path =
                                            steamapps.join("common").join(installdir);
                                        env.steam_apps.insert(
                                            appid.to_string(),
                                            install_path.to_string_lossy().to_string(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                    let common = format!("{root}\\steamapps\\common");
                    if existing(&common).is_some()
                        && !install_paths.iter().any(|r| r.path == common)
                    {
                        install_paths.push(InstallRoot { path: common, platform: "Steam".into() });
                    }
                }
            }
        }
        // 账号：loginusers.vdf
        let login_users = std::path::Path::new(steam).join("config").join("loginusers.vdf");
        if let Ok(text) = std::fs::read_to_string(&login_users) {
            if let Ok(vdf) = crate::vdf::parse_vdf(&text) {
                if let Some((id64, account_id, account_name, persona)) =
                    crate::vdf::steam_autologin_account(&vdf)
                {
                    env.accounts.steam = Some(account_id);
                    let _ = (id64, account_name, persona); // 64 位 ID 与昵称留给 UI 层
                }
            }
        }
    }

    // ---- Epic ----
    if let Some(programdata) = std::env::var_os("PROGRAMDATA") {
        let manifest = std::path::PathBuf::from(programdata)
            .join("Epic")
            .join("UnrealEngineLauncher")
            .join("LauncherInstalled.dat");
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            for root in epic_install_roots_from_json(&text) {
                push_root(&mut install_paths, &root, "Epic");
            }
        }
    }
    // Epic 账号：LOCALAPPDATA\EpicGamesLauncher\Saved\Data\*.dat，取最近修改
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let data_dir = std::path::PathBuf::from(local)
            .join("EpicGamesLauncher")
            .join("Saved")
            .join("Data");
        if let Ok(entries) = std::fs::read_dir(&data_dir) {
            let mut candidates: Vec<(String, u64)> = Vec::new();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(account) = epic_account_from_filename(&name) {
                    let mtime = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    candidates.push((account, mtime));
                }
            }
            if let Some(account) = latest_account_from_entries(&candidates) {
                env.accounts.epic = Some(account);
            }
        }
    }

    // ---- Ubisoft ----
    let ubisoft_path = reg_get_string(
        winreg::enums::HKEY_LOCAL_MACHINE,
        r"SOFTWARE\WOW6432Node\Ubisoft\Launcher",
        "InstallDir",
    );
    if let Some(ubisoft) = &ubisoft_path {
        env.ubisoft_path = Some(ubisoft.clone());
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let yaml_path = std::path::PathBuf::from(local)
            .join("Ubisoft Game Launcher")
            .join("settings.yaml");
        if let Ok(text) = std::fs::read_to_string(&yaml_path) {
            if let Some(root) = ubisoft_install_root_from_yaml(&text) {
                push_root(&mut install_paths, &root, "Ubisoft");
            }
        }
        // Ubisoft 账号：savegames 目录下最近修改的纯数字目录
        if let Some(ubisoft) = &ubisoft_path {
            let savegames = std::path::Path::new(ubisoft).join("savegames");
            if let Ok(entries) = std::fs::read_dir(&savegames) {
                let mut candidates: Vec<(String, u64)> = Vec::new();
                for entry in entries.flatten() {
                    if entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                        let name = entry.file_name().to_string_lossy().to_string();
                        let mtime = entry
                            .metadata()
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        candidates.push((name, mtime));
                    }
                }
                // 先过滤出纯数字目录再取最新：savegames 下混入非数字目录（如 .backup）
                // 时不能让它们抢掉真实账号
                let numeric: Vec<(String, u64)> = candidates
                    .into_iter()
                    .filter(|(name, _)| !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()))
                    .collect();
                if let Some(account) = latest_account_from_entries(&numeric) {
                    env.accounts.ubisoft = Some(account);
                }
            }
        }
    }

    // ---- EA ----
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let ea_dir = std::path::PathBuf::from(local).join("Electronic Arts").join("EA Desktop");
        if let Ok(entries) = std::fs::read_dir(&ea_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(account) = ea_account_from_filename(&name) {
                    env.accounts.ea = Some(account);
                }
                if name.starts_with("user_") && name.ends_with(".ini") {
                    if let Ok(text) = std::fs::read_to_string(entry.path()) {
                        for root in ea_install_roots_from_ini(&text) {
                            push_root(&mut install_paths, &root, "EA");
                        }
                    }
                }
            }
        }
    }

    // ---- GOG ----
    if let Some(programdata) = std::env::var_os("PROGRAMDATA") {
        let config = std::path::PathBuf::from(programdata)
            .join("GOG.com")
            .join("Galaxy")
            .join("config.json");
        if let Ok(text) = std::fs::read_to_string(&config) {
            if let Some(root) = gog_install_root_from_json(&text) {
                push_root(&mut install_paths, &root, "GOG");
            }
        }
    }
    env.accounts.gog = reg_get_string(
        winreg::enums::HKEY_CURRENT_USER,
        r"Software\GOG.com\Galaxy\settings",
        "userId",
    );

    // ---- Battle.net ----
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let config = std::path::PathBuf::from(appdata)
            .join("Battle.net")
            .join("Battle.net.config");
        if let Ok(text) = std::fs::read_to_string(&config) {
            if let Some(root) = battlenet_install_root_from_json(&text) {
                push_root(&mut install_paths, &root, "Battle.net");
            }
        }
    }

    // ---- EA / Rockstar 默认库根（启动器不提供配置文件时的兜底） ----
    for (root, platform) in [
        ("C:\\Program Files\\EA Games", "EA"),
        ("C:\\Program Files (x86)\\EA Games", "EA"),
        ("C:\\Program Files\\Rockstar Games", "Rockstar"),
        ("C:\\Program Files (x86)\\Rockstar Games", "Rockstar"),
    ] {
        push_root(&mut install_paths, root, platform);
    }

    // ---- 注册表卸载项扫描：兜底发现各启动器安装的游戏目录 ----
    // 卸载项的 InstallLocation 是游戏目录本身，取其父目录作为库根并按发布者标记平台
    for (hive, key_root) in [
        (winreg::enums::HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (winreg::enums::HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (winreg::enums::HKEY_CURRENT_USER, r"Software\Microsoft\Windows\CurrentVersion\Uninstall"),
    ] {
        let Ok(uninstall) = winreg::RegKey::predef(hive).open_subkey(key_root) else {
            continue;
        };
        for sub in uninstall.enum_keys().flatten() {
            let Ok(entry) = uninstall.open_subkey(&sub) else { continue };
            let publisher = entry
                .get_value::<String, _>("Publisher")
                .unwrap_or_default();
            let platform = match publisher.to_lowercase() {
                p if p.contains("electronic arts") || p.contains("ea ") => "EA",
                p if p.contains("rockstar") => "Rockstar",
                p if p.contains("ubisoft") => "Ubisoft",
                p if p.contains("epic") => "Epic",
                p if p.contains("gog") => "GOG",
                p if p.contains("blizzard") => "Blizzard",
                _ => continue, // 只补平台游戏，避免把所有软件都当成库根
            };
            let location = entry
                .get_value::<String, _>("InstallLocation")
                .unwrap_or_default();
            if location.is_empty() {
                continue;
            }
            // InstallLocation = 游戏目录本身 → 父目录作为库根
            let path = std::path::Path::new(&location);
            if let Some(parent) = path.parent() {
                if parent.parent().is_some() {
                    push_root(&mut install_paths, &parent.to_string_lossy(), platform);
                }
            }
        }
    }

    // ---- Xbox / Rockstar 账号 ----
    env.accounts.xbox = reg_get_string(
        winreg::enums::HKEY_CURRENT_USER,
        r"Software\Microsoft\XboxLive",
        "Xuid",
    );
    if let Some(userprofile) = std::env::var_os("USERPROFILE") {
        let profiles = std::path::PathBuf::from(userprofile)
            .join("Documents")
            .join("Rockstar Games")
            .join("Social Club")
            .join("Profiles");
        if let Ok(entries) = std::fs::read_dir(&profiles) {
            let mut candidates: Vec<(String, u64)> = Vec::new();
            for entry in entries.flatten() {
                if entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let mtime = entry
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    candidates.push((name, mtime));
                }
            }
            if let Some(account) = latest_account_from_entries(&candidates) {
                env.accounts.rockstar = Some(account);
            }
        }
    }

    env.install_paths = install_paths;
    env
}

#[cfg(target_os = "windows")]
fn existing(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .exists()
        .then(|| path.to_string())
}

#[cfg(target_os = "windows")]
fn push_root(list: &mut Vec<InstallRoot>, path: &str, platform: &str) {
    if let Some(existing) = existing(path) {
        if !list.iter().any(|r| r.path == existing) {
            list.push(InstallRoot { path: existing, platform: platform.to_string() });
        }
    }
}

#[cfg(target_os = "windows")]
fn reg_get_string(hive: winreg::HKEY, subkey: &str, value: &str) -> Option<String> {
    let key = winreg::RegKey::predef(hive)
        .open_subkey(subkey)
        .ok()?;
    key.get_value(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epic_roots_take_parents_deduped() {
        let json = r#"{"InstallationList":[
            {"InstallLocation":"D:\\Epic Games\\GameA"},
            {"InstallLocation":"D:\\Epic Games\\GameB"},
            {"InstallLocation":"E:\\Store\\Other"}
        ]}"#;
        let roots = epic_install_roots_from_json(json);
        assert_eq!(roots.len(), 2);
        assert!(roots.contains(&"D:\\Epic Games".to_string()));
        assert!(roots.contains(&"E:\\Store".to_string()));
    }

    #[test]
    fn gog_and_battlenet_roots() {
        assert_eq!(
            gog_install_root_from_json(r#"{"libraryPath":"D:\\GOG Library"}"#).unwrap(),
            "D:\\GOG Library"
        );
        assert_eq!(
            battlenet_install_root_from_json(
                r#"{"Client":{"Install":{"DefaultInstallPath":"D:\\Battle.net"}}}"#
            )
            .unwrap(),
            "D:\\Battle.net"
        );
    }

    #[test]
    fn ea_ini_and_account() {
        let ini = "user.downloadinplacedir=D:\\EA Games\r\nuser.other=x";
        assert_eq!(ea_install_roots_from_ini(ini), vec!["D:\\EA Games"]);
        assert_eq!(ea_account_from_filename("user_100923487.ini").unwrap(), "100923487");
        assert!(ea_account_from_filename("readme.txt").is_none());
    }

    #[test]
    fn ubisoft_yaml_and_latest_account() {
        let yaml = "misc:\n  game_installation_path: \"D:\\Ubisoft\"\n  other: 1";
        assert_eq!(
            ubisoft_install_root_from_yaml(yaml).unwrap(),
            "D:\\Ubisoft"
        );
        let entries = vec![
            ("123".to_string(), 100u64),
            ("456".to_string(), 200u64),
        ];
        assert_eq!(latest_account_from_entries(&entries).unwrap(), "456");
    }

    #[test]
    fn epic_account_from_filename_variants() {
        assert_eq!(epic_account_from_filename("OC_deadbeef.dat").unwrap(), "deadbeef");
        assert_eq!(epic_account_from_filename("deadbeef.dat").unwrap(), "deadbeef");
        assert!(epic_account_from_filename("junk.txt").is_none());
    }
}
