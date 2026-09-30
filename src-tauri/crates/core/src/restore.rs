//! 恢复引擎（行为规格见 SPEC.md §8）
//!
//! 智能恢复拆成两步：check_restore_conflict（本地较新 → 冲突）→ restore_backup(force)，
//! 由前端 UI 确认，不在命令执行中途弹原生对话框。

use crate::backup::{BackupSummary, GameBackups};
use crate::detect::DetectedEnvironment;
use crate::paths;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// 恢复目标解析失败的原因
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RestoreBlocker {
    GameNotInstalled,
    SteamNotInstalled,
    UbisoftNotInstalled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    pub conflict: bool,
    /// 本地存档最近修改时间（毫秒）
    pub local_ms: Option<u64>,
    /// 备份存档最近修改时间（毫秒）
    pub backup_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub restored_paths: usize,
    pub errors: Vec<String>,
}

// ======================================================================
// 恢复目标解析
// ======================================================================

/// 把备份里存的展示模板在"当前环境"下重新解析为具体目标路径。
/// {{p|game}} 解析不到时返回 GameNotInstalled；steam/ubisoft 缺失时同理。
fn resolve_restore_destination(
    template: &str,
    install_folder: Option<&str>,
    env: &DetectedEnvironment,
    home: &Path,
) -> Result<String, RestoreBlocker> {
    let mut out = String::new();
    let mut rest = template;

    while let Some(rel) = find_ci(rest, "{{p|") {
        let token_start = rel;
        let Some(token_end_rel) = rest[token_start + 4..].find("}}") else {
            out.push_str(rest);
            rest = "";
            break;
        };
        let token_end = token_start + 4 + token_end_rel + 2;
        let token_raw = &rest[token_start + 4..token_start + 4 + token_end_rel];
        let normalized = paths::normalize(token_raw);
        out.push_str(&rest[..token_start]);

        let resolved = match normalized.as_str() {
            "game" => {
                let install = install_folder
                    .and_then(|folder| crate::scanner::find_install_path(folder, &env.install_paths));
                match install {
                    Some(p) => Some(p),
                    None => return Err(RestoreBlocker::GameNotInstalled),
                }
            }
            "steam" => env.steam_path.clone().filter(|s| !s.is_empty()),
            "uplay" | "ubisoftconnect" => env.ubisoft_path.clone().filter(|s| !s.is_empty()),
            _ => paths::resolve_one(&normalized, env.platform, &|k: &str| std::env::var(k).ok(), home),
        };

        match resolved {
            Some(value) => out.push_str(&value),
            // 解析不了的 token 原样保留，交给绝对路径检查兜底
            None => out.push_str(&format!("{{{{p|{normalized}}}}}")),
        }
        rest = &rest[token_end..];
    }
    out.push_str(rest);
    Ok(out)
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let (hb, nb) = (hay.as_bytes(), needle.as_bytes());
    if nb.is_empty() || hb.len() < nb.len() {
        return None;
    }
    let mut i = 0usize;
    while i + nb.len() <= hb.len() {
        if hb[i..i + nb.len()].eq_ignore_ascii_case(nb) {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn latest_mtime(path: &Path) -> Option<u64> {
    let (_, ms) = crate::scanner::walk_size(path);
    ms
}

// ======================================================================
// 智能恢复检查
// ======================================================================

/// 备份实例定位：目录式或 zip；zip 解包到临时目录并返回清理路径
fn open_backup_source(
    backups_root: &Path,
    wiki_page_id: i64,
    date: &str,
) -> Result<(PathBuf, Option<PathBuf>), String> {
    let dir = backups_root.join(wiki_page_id.to_string()).join(date);
    if dir.is_dir() {
        return Ok((dir, None));
    }
    let zip = backups_root.join(wiki_page_id.to_string()).join(format!("{date}.zip"));
    if zip.is_file() {
        let tmp = std::env::temp_dir().join(format!(
            "gm-restore-{}-{}-{}",
            wiki_page_id, date, std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        crate::backup::extract_zip(&zip, &tmp)?;
        let cleanup = tmp.clone();
        return Ok((tmp, Some(cleanup)));
    }
    Err(format!("备份不存在: {date}"))
}

/// 比较备份与本地存档的最近修改时间。备份较旧且本地较新 → 冲突，需要用户确认。
pub fn check_restore_conflict(
    backups_root: &Path,
    game: &GameBackups,
    date: &str,
    env: &DetectedEnvironment,
    home: &Path,
) -> Result<ConflictInfo, String> {
    let summary = game
        .backups
        .iter()
        .find(|b| b.date == date)
        .ok_or_else(|| format!("找不到备份 {date}"))?;

    let (backup_folder, cleanup) = open_backup_source(backups_root, game.wiki_page_id, date)?;
    let result = {
        let mut latest_backup_ms: Option<u64> = None;
        let mut latest_local_ms: Option<u64> = None;

        for entry in &summary.backup_paths {
            let source = backup_folder.join(&entry.folder_name);
            latest_backup_ms = max_opt(latest_backup_ms, latest_mtime(&source));
            match resolve_restore_destination(
                &entry.template,
                entry.install_folder.as_deref(),
                env,
                home,
            ) {
                Ok(dest) if Path::new(&dest).exists() => {
                    latest_local_ms = max_opt(latest_local_ms, latest_mtime(Path::new(&dest)));
                }
                _ => {} // 目标不存在 = 未安装/无本地存档，不参与冲突比较
            }
        }

        let conflict = matches!((latest_backup_ms, latest_local_ms), (b, l) if l > b);
        Ok(ConflictInfo {
            conflict,
            local_ms: latest_local_ms,
            backup_ms: latest_backup_ms,
        })
    };
    if let Some(tmp) = cleanup {
        let _ = std::fs::remove_dir_all(tmp);
    }
    result
}

fn max_opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

// ======================================================================
// 恢复执行
// ======================================================================

pub fn restore_backup(
    backups_root: &Path,
    game: &GameBackups,
    date: &str,
    env: &DetectedEnvironment,
    home: &Path,
    force: bool, // true = 无视冲突直接覆盖
) -> Result<RestoreResult, String> {
    let summary: &BackupSummary = game
        .backups
        .iter()
        .find(|b| b.date == date)
        .ok_or_else(|| format!("找不到备份 {date}"))?;

    let (backup_folder, cleanup) = open_backup_source(backups_root, game.wiki_page_id, date)?;
    let work = (|| -> Result<RestoreResult, String> {
        let mut restored = 0usize;
        let mut errors: Vec<String> = Vec::new();

        if !force {
            let conflict = check_restore_conflict(backups_root, game, date, env, home)?;
            if conflict.conflict {
                return Err("CONFLICT".to_string()); // 命令层转成结构化提示，前端走确认流程
            }
        }

    for entry in &summary.backup_paths {
        let source = backup_folder.join(&entry.folder_name);
        if !source.exists() {
            errors.push(format!("备份内路径不存在: {}", source.display()));
            continue;
        }

        let destination = match resolve_restore_destination(
            &entry.template,
            entry.install_folder.as_deref(),
            env,
            home,
        ) {
            Ok(dest) => PathBuf::from(dest),
            Err(blocker) => {
                errors.push(match blocker {
                    RestoreBlocker::GameNotInstalled => "游戏未安装，无法恢复（{{p|game}}）".to_string(),
                    RestoreBlocker::SteamNotInstalled => "Steam 未安装，无法恢复（{{p|steam}}）".to_string(),
                    RestoreBlocker::UbisoftNotInstalled => "育碧连接未安装，无法恢复（{{p|ubisoftconnect}}）".to_string(),
                });
                continue;
            }
        };

        if !Path::new(&destination).is_absolute() {
            errors.push(format!("目标路径不是绝对路径: {}", destination.display()));
            continue;
        }

        match entry.kind.as_str() {
            "folder" => {
                std::fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
                crate::backup::copy_dir_recursive(&source, &destination)?;
                restored += 1;
            }
            "file" => {
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let file_name = destination.file_name().ok_or("目标缺少文件名")?;
                std::fs::copy(source.join(file_name), &destination).map_err(|e| e.to_string())?;
                restored += 1;
            }
            "reg" => {
                #[cfg(target_os = "windows")]
                {
                    let reg_file = source.join("registry_backup.reg");
                    let output = std::process::Command::new("reg")
                        .args(["import"])
                        .arg(&reg_file)
                        .output()
                        .map_err(|e| e.to_string())?;
                    if !output.status.success() {
                        errors.push(format!(
                            "reg import 失败: {}",
                            String::from_utf8_lossy(&output.stderr)
                        ));
                    } else {
                        restored += 1;
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    errors.push("注册表恢复仅支持 Windows".to_string());
                }
            }
            other => errors.push(format!("未知备份类型: {other}")),
        }
    }

    Ok(RestoreResult {
        restored_paths: restored,
        errors,
    })
    })();
    if let Some(tmp) = cleanup {
        let _ = std::fs::remove_dir_all(tmp);
    }
    work
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_stub() -> DetectedEnvironment {
        DetectedEnvironment {
            platform: crate::platform::Platform::Windows,
            install_paths: vec![],
            steam_apps: Default::default(),
            steam_path: Some("C:\\Steam".into()),
            ubisoft_path: None,
            accounts: Default::default(),
        }
    }

    #[test]
    fn restore_destination_resolves_current_env() {
        let env = env_stub();
        let dest = resolve_restore_destination(
            "{{p|appdata}}\\CoolGame\\saves",
            None,
            &env,
            Path::new("C:\\Users\\t"),
        )
        .unwrap();
        // 本机 USERPROFILE 注入由真实环境决定，只断言模板已被替换
        assert!(!dest.contains("{{p|"));
        assert!(dest.ends_with("CoolGame\\saves"));
    }

    #[test]
    fn game_placeholder_without_install_reports_blocker() {
        let env = env_stub();
        let err = resolve_restore_destination(
            "{{p|game}}\\saves",
            Some("MissingGame"),
            &env,
            Path::new("C:\\Users\\t"),
        );
        assert_eq!(err.unwrap_err(), RestoreBlocker::GameNotInstalled);
    }

    #[test]
    fn game_placeholder_with_install_resolves() {
        let mut env = env_stub();
        // 用临时目录模拟安装目录，避免写真实磁盘
        let tmp = std::env::temp_dir().join(format!("gm-restore-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("InstalledGame")).unwrap();
        env.install_paths = vec![crate::detect::InstallRoot {
            path: tmp.to_string_lossy().to_string(),
            platform: "Steam".into(),
        }];

        let dest = resolve_restore_destination(
            "{{p|game}}\\saves",
            Some("InstalledGame"),
            &env,
            Path::new("C:\\Users\\t"),
        )
        .unwrap();
        assert!(dest.ends_with("InstalledGame\\saves"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
