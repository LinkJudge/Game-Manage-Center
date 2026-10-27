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

/// 备份实例定位：目录式或 zip；zip 解包到临时目录并返回清理路径。
/// 游戏目录归属走 game_dir_for（游戏名布局 + game.json，旧数字布局兜底）。
fn open_backup_source(
    backups_root: &Path,
    wiki_page_id: i64,
    date: &str,
) -> Result<(PathBuf, Option<PathBuf>), String> {
    let date = crate::backup::validate_date(date)?;
    let base = crate::backup::game_dir_for(backups_root, wiki_page_id)
        .ok_or_else(|| format!("备份不存在: {date}"))?;
    let dir = base.join(date);
    if dir.is_dir() {
        return Ok((dir, None));
    }
    let zip = base.join(format!("{date}.zip"));
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

        // 注册表条目：目标是注册表键而非文件路径，直接导入备份的 .reg，
        // 不走文件路径解析/绝对路径检查（HKEY_* 会被误判为"不是绝对路径"）
        if entry.kind == "reg" {
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
            continue;
        }

        let destination = match resolve_restore_destination(
            &entry.template,
            entry.install_folder.as_deref(),
            env,
            home,
        ) {
            Ok(dest) => PathBuf::from(dest),
            Err(RestoreBlocker::GameNotInstalled) => {
                errors.push("游戏未安装，无法恢复（{{p|game}}）".to_string());
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
                let name = file_name.to_string_lossy();
                // 通配模板（如 *.sav）在备份时存的是具体文件：
                // 恢复时用实例内的真实文件名回填目标，而不是字面的 "*.sav"
                let (dest, src_file) = if name.contains('*') || name.contains('?') {
                    let actual = std::fs::read_dir(&source)
                        .map_err(|e| e.to_string())?
                        .flatten()
                        .find(|e| e.metadata().map(|m| m.is_file()).unwrap_or(false))
                        .ok_or_else(|| format!("备份内没有可恢复的文件: {}", source.display()))?;
                    let parent = destination.parent().ok_or("目标缺少父目录")?;
                    (parent.join(actual.file_name()), actual.path())
                } else {
                    (destination.clone(), source.join(file_name))
                };
                std::fs::copy(&src_file, &dest).map_err(|e| e.to_string())?;
                restored += 1;
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

    // ==================================================================
    // 集成回归：新布局（游戏名目录）下的恢复链路
    // ==================================================================

    use crate::database::{GameEntry, SaveLocations};
    use crate::resolve::{ResolvedKind, ResolvedPath};

    fn test_entry() -> GameEntry {
        GameEntry {
            title: "CoolGame".into(),
            wiki_page_id: 42,
            install_folder: Some("InstalledGame".into()),
            steam_id: None,
            gog_id: None,
            platforms: vec![],
            zh_cn: None,
            paths: SaveLocations::default(),
        }
    }

    /// 造一份新布局备份：返回 (root, backups_root, env, home)
    fn fixture(final_template: &str) -> (PathBuf, PathBuf, DetectedEnvironment, PathBuf) {
        let root = std::env::temp_dir().join(format!("gm-restore-it-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let src = root.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.sav"), b"savedata").unwrap();

        let install = root.join("install");
        std::fs::create_dir_all(install.join("InstalledGame")).unwrap();

        let resolved = vec![ResolvedPath {
            template: final_template.into(),
            final_template: final_template.into(),
            resolved: src.join("a.sav").to_string_lossy().to_string(),
            kind: ResolvedKind::File,
            content_kind: "saves".into(),
        }];
        let backups_root = root.join("backups");
        crate::backup::backup_game(&test_entry(), &resolved, &backups_root, 3, false).unwrap();

        let mut env = env_stub();
        env.install_paths = vec![crate::detect::InstallRoot {
            path: install.to_string_lossy().to_string(),
            platform: "Steam".into(),
        }];
        let home = root.join("home");
        (root, backups_root, env, home)
    }

    #[test]
    fn restore_works_with_game_name_layout() {
        // 回归：open_backup_source 曾按旧数字布局拼路径，新布局下恢复必报"备份不存在"
        let (root, backups_root, env, home) = fixture("{{p|game}}\\saves\\a.sav");
        let game = crate::backup::list_backups(&backups_root, 42).unwrap().unwrap();
        let date = game.backups[0].date.clone();
        let result = restore_backup(&backups_root, &game, &date, &env, &home, true).unwrap();
        assert_eq!(result.errors, Vec::<String>::new(), "恢复不应报错");
        assert_eq!(result.restored_paths, 1);
        let restored = root.join("install").join("InstalledGame").join("saves").join("a.sav");
        assert_eq!(std::fs::read(&restored).unwrap(), b"savedata");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_wildcard_template_uses_actual_file_name() {
        // 回归：通配模板（*.sav）恢复时曾把字面 "*.sav" 当文件名，永远找不到备份内文件
        let (root, backups_root, env, home) = fixture("{{p|game}}\\saves\\*.sav");
        let game = crate::backup::list_backups(&backups_root, 42).unwrap().unwrap();
        let date = game.backups[0].date.clone();
        let result = restore_backup(&backups_root, &game, &date, &env, &home, true).unwrap();
        assert_eq!(result.errors, Vec::<String>::new(), "恢复不应报错: {:?}", result.errors);
        assert_eq!(result.restored_paths, 1);
        let restored = root.join("install").join("InstalledGame").join("saves").join("a.sav");
        assert_eq!(std::fs::read(&restored).unwrap(), b"savedata");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "需要能真实调用 reg.exe 的环境（沙箱/CI 常拦截），手动 cargo test -- --ignored 跑"]
    fn restore_registry_entry_imports_reg_file() {
        // 回归：reg 条目曾被"目标路径不是绝对路径"拦截，注册表恢复整体失效
        let root = std::env::temp_dir().join(format!("gm-restore-reg-{}", std::process::id()));
        let instance = root.join("backups").join("RegGame").join("2024-01-01_10-00-00");
        let entry_dir = instance.join("注册表");
        std::fs::create_dir_all(&entry_dir).unwrap();
        // 仅含头部的空 .reg：reg import 成功且不改动任何键
        std::fs::write(entry_dir.join("registry_backup.reg"), "Windows Registry Editor Version 5.00\r\n").unwrap();
        std::fs::write(
            root.join("backups").join("RegGame").join("game.json"),
            r#"{"schema":"gm.game-index/1","wikiPageId":7,"title":"RegGame"}"#,
        )
        .unwrap();
        std::fs::write(
            instance.join("manifest.json"),
            r#"{"schema":1,"title":"RegGame","titleZh":null,"createdAt":"2024-01-01_10-00-00",
"size":null,"pinned":null,"label":null,
"entries":[{"dir":"注册表","template":"{{p|hkcu}}\\Software\\RegGame","store":"reg","installFolder":null,"contentType":"reg"}]}"#,
        )
        .unwrap();

        let env = env_stub();
        let game = crate::backup::list_backups(&root.join("backups"), 7).unwrap().unwrap();
        let result = restore_backup(&root.join("backups"), &game, "2024-01-01_10-00-00", &env, &root, true).unwrap();
        assert_eq!(result.errors, Vec::<String>::new(), "reg 恢复不应报错: {:?}", result.errors);
        assert_eq!(result.restored_paths, 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
