//! 快速/深度扫描引擎。
//!
//! 快速扫描的性能模型：
//! 1. 枚举所有游戏库根目录的直接子目录 → "目录名 → 路径"映射（首个根目录优先）
//! 2. 数据库按 install_folder IN (目录名) 查出真正已安装的游戏（分块查询，
//!    规避 SQLite 变量数上限）
//! 3. 只对已安装游戏做模板解析 + 大小统计，未安装游戏不跑 glob
//!
//! 单游戏扫描：每个模板解析出的具体路径若实测大小 > 0 则纳入；
//! reg 桶仅 Windows 参与（先出候选，键存在性由 scanner 过滤）。

use crate::database::{Database, GameEntry};
use crate::custom::CustomGame;
use crate::detect::{InstallRoot, DetectedEnvironment};
use crate::platform::Platform;
use crate::resolve::{
    resolve_registry_template, resolve_template, AccountIds, ResolveContext, ResolvedPath,
};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

/// 备份树自己的清单文件（当前与遗留格式），统计游戏存档大小时跳过
pub const SKIP_FILES: &[&str] = &["manifest.json", "backup_info.json"];

fn epoch_ms(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 递归统计目录大小与最近修改时间；路径不存在按零处理
pub fn walk_size(path: &Path) -> (u64, Option<u64>) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return (0, None);
    };
    if !metadata.is_dir() {
        return (metadata.len(), metadata.modified().ok().map(epoch_ms));
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return (0, None);
    };
    let mut total = 0u64;
    let mut latest: Option<u64> = None;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if SKIP_FILES.contains(&name.as_str()) {
            continue;
        }
        let (size, modified) = walk_size(&entry.path());
        total += size;
        if modified.is_some_and(|m| latest.is_none_or(|l| m > l)) {
            latest = modified;
        }
    }
    (total, latest)
}

/// 游戏库根目录 → 直接子目录 → (平台, 路径) 列表。
/// 同一游戏目录可同时出现在多个平台库下（多平台安装检测的依据）；
/// 同平台内首个根目录优先。
pub fn build_install_folder_map(
    roots: &[InstallRoot],
) -> HashMap<String, Vec<(String, String)>> {
    let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root.path) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                let name = entry.file_name().to_string_lossy().to_string();
                let list = map.entry(name).or_default();
                if !list.iter().any(|(p, _)| p == &root.platform) {
                    list.push((root.platform.clone(), entry.path().to_string_lossy().to_string()));
                }
            }
        }
    }
    map
}

/// 单游戏安装路径判定：首个命中的根目录
pub fn find_install_path(install_folder: &str, roots: &[InstallRoot]) -> Option<String> {
    find_installed(install_folder, roots).0
}

/// 单游戏安装检测：返回（首个安装路径, 全部安装平台，按检测顺序去重）
pub fn find_installed(
    install_folder: &str,
    roots: &[InstallRoot],
) -> (Option<String>, Vec<String>) {
    if install_folder.is_empty() {
        return (None, Vec::new());
    }
    let mut first_path: Option<String> = None;
    let mut platforms: Vec<String> = Vec::new();
    for root in roots {
        let candidate = Path::new(&root.path).join(install_folder);
        if candidate.exists() {
            if first_path.is_none() {
                first_path = Some(candidate.to_string_lossy().to_string());
            }
            if !platforms.contains(&root.platform) {
                platforms.push(root.platform.clone());
            }
        }
    }
    (first_path, platforms)
}

/// 目录里是否至少有一个真实文件（浅递归）。
/// 用于剔除"卸载后残留的空壳文件夹"——目录还在但游戏文件已清空
fn dir_has_any_file(dir: &Path, depth: u32) -> bool {
    if depth > 4 {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() {
            return true;
        }
        if p.is_dir() && dir_has_any_file(&p, depth + 1) {
            return true;
        }
    }
    false
}

/// 带核验的安装检测（扫描用，剔除卸载残留误判）：
/// - Steam：已知 steam_id 时以 appmanifest 为准——卸载时 Steam 会删掉 acf，
///   之后残留在 steamapps/common 的同名文件夹不再算作已安装；
/// - 其他平台 / 未知 steam_id：文件夹必须真实含文件（空目录树不算已安装）
pub fn find_installed_verified(
    install_folder: &str,
    roots: &[InstallRoot],
    steam_id: Option<&str>,
    steam_apps: &HashMap<String, String>,
) -> (Option<String>, Vec<String>) {
    if install_folder.is_empty() {
        return (None, Vec::new());
    }
    let mut first_path: Option<String> = None;
    let mut platforms: Vec<String> = Vec::new();

    // Steam：appmanifest 权威判定（acf 在 = 装了，acf 删了 = 卸了）
    if let Some(sid) = steam_id {
        if let Some(p) = steam_apps.get(sid) {
            if Path::new(p).exists() {
                first_path = Some(p.clone());
                platforms.push("Steam".to_string());
            }
        }
    }

    for root in roots {
        // 已知 steam_id 的游戏在 Steam 库目录上只认 appmanifest，
        // 不走文件夹存在性兜底——否则卸载残留文件夹必然误判
        if root.platform == "Steam" && steam_id.is_some() {
            continue;
        }
        let candidate = Path::new(&root.path).join(install_folder);
        if candidate.is_dir() && dir_has_any_file(&candidate, 0) {
            if first_path.is_none() {
                first_path = Some(candidate.to_string_lossy().to_string());
            }
            if !platforms.contains(&root.platform) {
                platforms.push(root.platform.clone());
            }
        }
    }
    (first_path, platforms)
}

/// 扫描结果：一个游戏
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedGame {
    pub entry: GameEntry,
    pub install_path: Option<String>,
    /// 实际安装了该游戏的平台（多平台都装了则有多个）
    pub installed_platforms: Vec<String>,
    pub resolved_files: Vec<ResolvedPath>,
    pub registry_candidates: Vec<ResolvedPath>,
    pub backup_size: u64,
    pub latest_modified_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutput {
    pub games: Vec<ScannedGame>,
    pub scanned_roots: usize,
    pub detected_folders: usize,
    /// true = 深度扫描（含已卸载游戏的残留存档）
    pub deep: bool,
}

/// 注册表键是否存在（reg.exe 导出的前置校验；非 Windows 恒 false）
pub fn registry_key_exists(path: &str) -> bool {    #[cfg(target_os = "windows")]
    {
        let (hive_str, subkey) = match path.split_once('\\') {
            Some(v) => v,
            None => return false,
        };
        let hive = match hive_str.to_uppercase().as_str() {
            "HKEY_CURRENT_USER" => winreg::enums::HKEY_CURRENT_USER,
            "HKEY_LOCAL_MACHINE" => winreg::enums::HKEY_LOCAL_MACHINE,
            _ => return false,
        };
        winreg::RegKey::predef(hive)
            .open_subkey(subkey)
            .is_ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        false
    }
}

/// 过宽模板防护：解析结果恰好是用户根目录（主目录/Documents/AppData 各级/
/// ProgramData 等）的模板属于 wiki 垃圾数据（如 Nethergate 的"整个 Documents"、
/// Oregon Trail II 的 Documents 根）——备份它们等于全盘搬运，直接跳过。
pub fn is_protected_root(path: &str, platform: Platform, home: &Path) -> bool {
    const PROTECTED: &[&str] = &[
        "userprofile",
        "userprofile/documents",
        "userprofile/appdata/locallow",
        "appdata",
        "localappdata",
        "programdata",
        "public",
        "windir",
        "programfiles",
    ];
    let norm = |p: &str| {
        p.replace('/', "\\")
            .to_lowercase()
            .trim_end_matches(['\\', '/'])
            .to_string()
    };
    let target = norm(path);
    if target.is_empty() {
        return true;
    }
    PROTECTED.iter().any(|token| {
        crate::paths::resolve_one(token, platform, &|k| std::env::var(k).ok(), home)
            .map(|p| norm(&p) == target)
            .unwrap_or(false)
    })
}

/// 合并规则（SPEC §3）：一条解析路径是另一条的子目录（或与另一条相同）时丢弃，
/// 只保留父目录——数据库常给同一游戏同时登记父目录与子目录模板
/// （如 Dying Light 的 `userdata\<uid>\239140\` 与 `...\239140\remote\out\`），
/// 不去重会导致子路径内容被备份两次、大小重复计入。
/// Windows 路径大小写不敏感，比较前统一小写并去掉尾分隔符。
fn dedup_subpaths(paths: &mut Vec<ResolvedPath>) {
    if paths.len() < 2 {
        return;
    }
    let norm = |p: &str| {
        p.replace('/', "\\")
            .to_lowercase()
            .trim_end_matches('\\')
            .to_string()
    };
    // 短路径在前，保证父目录先入 kept、子路径随后被支配判定命中
    paths.sort_by(|a, b| {
        let (na, nb) = (norm(&a.resolved), norm(&b.resolved));
        na.len().cmp(&nb.len()).then_with(|| na.cmp(&nb))
    });
    let mut kept: Vec<(String, ResolvedPath)> = Vec::with_capacity(paths.len());
    for p in paths.drain(..) {
        let n = norm(&p.resolved);
        let dominated = kept.iter().any(|(k, _)| n == *k || n.starts_with(&format!("{k}\\")));
        if !dominated {
            kept.push((n, p));
        }
    }
    *paths = kept.into_iter().map(|(_, p)| p).collect();
}

/// 单游戏完整解析：文件桶按当前平台，reg 桶仅 Windows
pub fn scan_game(
    entry: &GameEntry,
    env: &DetectedEnvironment,
    home: &Path,
    all_accounts: bool,
) -> ScannedGame {
    let (install_path, installed_platforms) = entry
        .install_folder
        .as_deref()
        .map(|folder| {
            find_installed_verified(
                folder,
                &env.install_paths,
                entry.steam_id.as_deref(),
                &env.steam_apps,
            )
        })
        .unwrap_or((None, Vec::new()));

    // {{p|game}} 的解析目录与"是否已安装"解耦：未判安装时回退到残留的同名文件夹——
    // 卸载后留在安装目录里的存档仍能扫到（模板只匹配真实存在的文件，
    // 文件夹真为空时不会解析出任何结果，不会误判）
    let game_dir = install_path.clone().or_else(|| {
        entry
            .install_folder
            .as_deref()
            .and_then(|f| find_install_path(f, &env.install_paths))
    });

    let accounts: AccountIds = env.accounts.clone();
    let steam_path = env.steam_path.clone();
    let ubisoft_path = env.ubisoft_path.clone();
    let ctx = ResolveContext {
        platform: env.platform,
        env: &|key: &str| std::env::var(key).ok(),
        home,
        game_dir,
        steam_dir: steam_path,
        ubisoft_dir: ubisoft_path,
        accounts: &accounts,
        all_accounts,
    };

    let mut resolved_files = Vec::new();
    let mut registry_candidates = Vec::new();
    let mut backup_size = 0u64;
    let mut latest_modified_ms: Option<u64> = None;

    let (bucket, kinds): (&[String], &[String]) = match env.platform {
        Platform::Windows => (&entry.paths.win, &entry.paths.win_kinds),
        Platform::MacOS => (&entry.paths.mac, &entry.paths.mac_kinds),
        Platform::Linux => (&entry.paths.linux, &entry.paths.linux_kinds),
    };
    // 先收集全部模板的解析结果，再合并（大小统计必须在去重之后，否则子路径重复计入）
    let mut candidates: Vec<ResolvedPath> = Vec::new();
    for (idx, template) in bucket.iter().enumerate() {
        // 类型与路径数组平行（saves/config），缺失时按存档兜底
        let content_kind = kinds.get(idx).cloned().unwrap_or_else(|| "saves".to_string());
        for mut resolved in resolve_template(template, &ctx) {
            // 过宽模板防护：整目录垃圾模板（Documents 根等）直接跳过
            if is_protected_root(&resolved.resolved, env.platform, home) {
                continue;
            }
            resolved.content_kind = content_kind.clone();
            candidates.push(resolved);
        }
    }
    dedup_subpaths(&mut candidates);
    for resolved in candidates {
        let (size, modified) = walk_size(Path::new(&resolved.resolved));
        if size > 0 {
            backup_size += size;
            if modified.is_some_and(|m| latest_modified_ms.is_none_or(|l| m > l)) {
                latest_modified_ms = modified;
            }
            resolved_files.push(resolved);
        }
    }

    if env.platform.supports_registry() {
        for template in &entry.paths.reg {
            for mut resolved in resolve_registry_template(template, &ctx) {
                // 键不存在的候选无法导出（reg.exe 会失败），直接过滤
                if registry_key_exists(&resolved.resolved) {
                    resolved.content_kind = "reg".to_string();
                    registry_candidates.push(resolved);
                }
            }
        }
    }

    ScannedGame {
        entry: entry.clone(),
        install_path,
        installed_platforms,
        resolved_files,
        registry_candidates,
        backup_size,
        latest_modified_ms,
    }
}

/// 确保 entry.install_folder 可用于匹配：
/// 1) 已命中库目录则不动；2) 有 steam_id 且 appmanifest 能定位 → 用其目录名；
/// 3) 否则用"标题≈目录名"兜底。
fn ensure_install_folder(
    entry: &mut GameEntry,
    folder_keys: &[&str],
    steam_apps: &HashMap<String, String>,
) {
    if let Some(f) = entry.install_folder.as_deref() {
        if folder_keys.iter().any(|k| k.eq_ignore_ascii_case(f)) {
            return;
        }
    }
    if let Some(sid) = entry.steam_id.as_deref() {
        if let Some(p) = steam_apps.get(sid) {
            if let Some(name) = Path::new(p).file_name() {
                entry.install_folder = Some(name.to_string_lossy().to_string());
                return;
            }
        }
    }
    if entry.install_folder.is_none() {
        entry.install_folder = Some(entry.title.clone());
    }
}

/// 快速扫描：已安装游戏全量解析（对应 getGameDataFromDB 快速模式）+ 自定义游戏
pub fn quick_scan(
    db: &Database,
    env: &DetectedEnvironment,
    home: &Path,
    all_accounts: bool,
    customs: &[CustomGame],
) -> Result<ScanOutput, String> {
    let folder_map = build_install_folder_map(&env.install_paths);
    let folder_keys: Vec<&str> = folder_map.keys().map(String::as_str).collect();
    let folders: Vec<&str> = folder_keys.clone();

    let mut games = Vec::new();
    let mut matched_folders: Vec<String> = Vec::new();
    for chunk in folders.chunks(500) {
        for mut entry in db.list_games_by_install_folder(chunk)? {
            ensure_install_folder(&mut entry, &folder_keys, &env.steam_apps);
            if let Some(f) = &entry.install_folder {
                matched_folders.push(f.clone());
            }
            games.push(scan_game(&entry, env, home, all_accounts));
        }
    }

    // EA/R星等 install_folder 缺失或与目录名不一致的游戏：按"标题≈目录名"兜底匹配
    let unmatched: Vec<&str> = folders
        .iter()
        .filter(|f| !matched_folders.iter().any(|m| m.eq_ignore_ascii_case(f)))
        .copied()
        .collect();
    for chunk in unmatched.chunks(500) {
        for mut entry in db.list_games_by_titles(chunk)? {
            ensure_install_folder(&mut entry, &folder_keys, &env.steam_apps);
            games.push(scan_game(&entry, env, home, all_accounts));
        }
    }

    // 兜底匹配可能与主匹配命中同一游戏（另一目录名恰好等于其标题），按 id 去重保序
    let mut seen_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();
    games.retain(|g| seen_ids.insert(g.entry.pcgw_id));

    // 自定义游戏（负数 id）：直接按用户提供的路径扫描
    for custom in customs {
        let entry = crate::custom::to_game_entry(custom);
        let mut scanned = scan_game(&entry, env, home, all_accounts);
        if scanned.installed_platforms.is_empty() {
            scanned.installed_platforms = vec!["自定义".into()];
        }
        games.push(scanned);
    }

    games.sort_by_key(|a| a.entry.title.to_lowercase());

    Ok(ScanOutput {
        scanned_roots: env.install_paths.len(),
        detected_folders: folder_map.len(),
        games,
        deep: false,
    })
}

/// 深度扫描：全库逐条解析（含已卸载游戏的残留存档）。
/// {{p|game}} 类模板因安装目录未命中自动跳过，只有真实存在的
/// AppData/文档/Steam 云等路径会浮出。注册表键做存在性过滤。
/// 5 万行多线程并行，耗时约 1-3 分钟。
pub fn deep_scan(
    db: &Database,
    env: &DetectedEnvironment,
    home: &Path,
    all_accounts: bool,
    customs: &[CustomGame],
) -> Result<ScanOutput, String> {
    let mut all: Vec<GameEntry> = db.list_all_games()?;
    let folder_map = build_install_folder_map(&env.install_paths);
    let folder_keys: Vec<&str> = folder_map.keys().map(String::as_str).collect();
    for entry in all.iter_mut() {
        ensure_install_folder(entry, &folder_keys, &env.steam_apps);
    }
    for custom in customs {
        all.push(crate::custom::to_game_entry(custom));
    }

    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8);
    let chunk_size = all.len().div_ceil(workers);
    let results: Vec<ScannedGame> = std::thread::scope(|s| {
        let mut handles = Vec::new();
        for chunk in all.chunks(chunk_size) {
            handles.push(s.spawn(move || {
                let mut out = Vec::new();
                for entry in chunk {
                    let scanned = scan_game(entry, env, home, all_accounts);
                    if !scanned.resolved_files.is_empty() || !scanned.registry_candidates.is_empty()
                    {
                        out.push(scanned);
                    }
                }
                out
            }));
        }
        handles
            .into_iter()
            .flat_map(|h| {
                h.join().unwrap_or_else(|_| {
                    // 线程 panic 时保留其余分片结果，并在控制台留下痕迹（不能静默丢游戏）
                    eprintln!("[gm-core] 深扫工作线程异常退出，该分片结果缺失");
                    Vec::new()
                })
            })
            .collect()
    });

    let mut games: Vec<ScannedGame> = results;
    games.sort_by_key(|a| a.entry.title.to_lowercase());

    Ok(ScanOutput {
        scanned_roots: env.install_paths.len(),
        detected_folders: games.len(),
        games,
        deep: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::SaveLocations;
    use std::fs;
    use std::path::PathBuf;

    fn write_file(path: &Path, size: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, vec![b'x'; size]).unwrap();
    }

    #[test]
    fn walk_size_sums_and_skips_manifests() {
        let root = std::env::temp_dir().join(format!("gm-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_file(&root.join("saves/a.sav"), 100);
        write_file(&root.join("saves/sub/b.sav"), 50);
        write_file(&root.join("manifest.json"), 99999);
        write_file(&root.join("backup_info.json"), 88888);

        let (size, latest) = walk_size(&root);
        assert_eq!(size, 150); // 清单文件（当前+遗留）被跳过
        assert!(latest.is_some());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn walk_size_missing_path_is_zero() {
        let (size, latest) = walk_size(Path::new("Z:/definitely/not/here"));
        assert_eq!(size, 0);
        assert!(latest.is_none());
    }

    #[test]
    fn install_folder_map_multi_platform() {
        let root = std::env::temp_dir().join(format!("gm-folders-{}", std::process::id()));
        write_file(&root.join("lib1/GameA/save.txt"), 1);
        write_file(&root.join("lib2/GameA/other.txt"), 1);
        write_file(&root.join("lib2/GameB/save.txt"), 1);
        write_file(&root.join("lib3/GameA/again.txt"), 1);

        let map = build_install_folder_map(&[
            InstallRoot { path: root.join("lib1").to_string_lossy().to_string(), platform: "Steam".into() },
            InstallRoot { path: root.join("lib2").to_string_lossy().to_string(), platform: "GOG".into() },
            InstallRoot { path: root.join("lib3").to_string_lossy().to_string(), platform: "Epic".into() },
        ]);
        // GameA 出现在 Steam(lib1)、GOG(lib2)、Epic(lib3) 三个平台库下
        assert_eq!(map["GameA"].len(), 3);
        assert_eq!(map["GameA"][0].0, "Steam");
        assert!(map["GameA"][0].1.contains("lib1"));
        assert!(map["GameA"].iter().any(|(p, _)| p == "GOG"));
        assert!(map["GameA"].iter().any(|(p, _)| p == "Epic"));
        assert!(map["GameB"].iter().any(|(_, path)| path.contains("lib2")));
        let _ = fs::remove_dir_all(&root);
    }

    /// 真实机器集成测试（不进 CI）：cargo test -p gm-core -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_machine_scan_smoke() {
        // 本测试位于 crates/core，数据库在 src-tauri/resources
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/database.db");
        let db = Database::open_readonly(&path).unwrap();
        let env = crate::detect::detect();
        println!("平台: {:?}", env.platform);
        println!("库根目录: {:?}", env.install_paths);
        println!("steam_apps 条目数: {}", env.steam_apps.len());
        println!(
            "账号: steam={:?} ubisoft={:?} epic={:?} xbox={:?} gog={:?} ea={:?}",
            env.accounts.steam, env.accounts.ubisoft, env.accounts.epic,
            env.accounts.xbox, env.accounts.gog, env.accounts.ea
        );
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap();
        let output = quick_scan(&db, &env, &home, false, &[]).unwrap();
        let with_files = output.games.iter().filter(|g| !g.resolved_files.is_empty()).count();
        let with_reg = output.games.iter().filter(|g| !g.registry_candidates.is_empty()).count();
        println!(
            "扫描: {} 个目录 / {} 个游戏命中 install_folder / {} 个有可用存档 / {} 个有注册表候选",
            output.detected_folders,
            output.games.len(),
            with_files,
            with_reg
        );
        for game in output.games.iter().filter(|g| !g.resolved_files.is_empty()) {
            println!(
                "  - {} ({} | {}) [{}] install={:?} {}",
                game.entry.title,
                game.entry.zh_cn.as_deref().unwrap_or("-"),
                format_bytes(game.backup_size),
                game.installed_platforms.join("+"),
                game.install_path,
                game.resolved_files.first().map(|r| r.resolved.clone()).unwrap_or_default()
            );
            for r in game.resolved_files.iter().skip(1) {
                println!("      + {}", r.resolved);
            }
        }
        // 命中安装但没解析出任何文件的：安装检测对、存档模板失败，诊断专用
        for game in output.games.iter().filter(|g| g.resolved_files.is_empty() && g.registry_candidates.is_empty()) {
            println!(
                "  ? {}（install: {:?}，模板全部未命中）",
                game.entry.title,
                game.install_path.as_deref().unwrap_or("?")
            );
        }
        for game in output.games.iter().filter(|g| !g.registry_candidates.is_empty()).take(5) {
            println!(
                "  [reg] {} → {}",
                game.entry.title,
                game.registry_candidates.first().map(|r| r.resolved.clone()).unwrap_or_default()
            );
        }
    }

    fn format_bytes(bytes: u64) -> String {
        if bytes < 1024 { return format!("{bytes} B"); }
        if bytes < 1024 * 1024 { return format!("{:.1} KB", bytes as f64 / 1024.0); }
        if bytes < 1024 * 1024 * 1024 { return format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)); }
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }

    #[test]
    fn dedup_subpaths_drops_child_and_duplicates() {
        // 数据库真实案例：Dying Light 同时登记父目录 239140\ 与子目录 ...\239140\remote\out\，
        // 子路径不去重会双份备份、大小双计（SPEC §3）
        let mk = |resolved: &str, kind: &str| ResolvedPath {
            template: resolved.into(),
            final_template: resolved.into(),
            resolved: resolved.into(),
            kind: crate::resolve::ResolvedKind::File,
            content_kind: kind.into(),
        };
        let mut paths = vec![
            mk(r"C:\U\239140\remote\out", "saves"),
            mk(r"C:\u\239140", "saves"),           // 父目录（大小写不同）
            mk(r"C:\U\239140\remote\out\", "saves"), // 子路径 + 尾分隔符（等价于第一条）
        ];
        dedup_subpaths(&mut paths);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].resolved, r"C:\u\239140"); // 保留父目录

        // 无包含关系的多条路径互不影响
        let mut paths = vec![mk(r"C:\A", "saves"), mk(r"C:\B", "config")];
        dedup_subpaths(&mut paths);
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn glob_trailing_slash_template_matches_directory() {
        // 数据库真实案例：BF1 模板以 "\settings\" 结尾（目录+尾分隔符），必须能匹配
        let root = std::env::temp_dir().join(format!("gm-tslash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_file(&root.join("AppData/Roaming/Battlefield 1/settings/video.cfg"), 10);

        let accounts = AccountIds::default();
        let no_env = |_: &str| -> Option<String> { None };
        let ctx = ResolveContext {
            platform: Platform::Windows,
            env: &no_env,
            home: &root,
            game_dir: None,
            steam_dir: None,
            ubisoft_dir: None,
            accounts: &accounts,
            all_accounts: false,
        };
        let results = resolve_template("{{p|appdata}}\\Battlefield 1\\settings\\", &ctx);
        assert_eq!(results.len(), 1, "尾分隔符目录模板必须匹配: {:?}", results);
        assert!(results[0].resolved.contains("settings"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_game_resolves_templates_and_sizes() {
        let root = std::env::temp_dir().join(format!("gm-game-{}", std::process::id()));
        // 游戏库根目录 lib/ 下的 CoolGame（install_folder 命中）
        write_file(&root.join("lib/CoolGame/saves/s1.sav"), 120);
        write_file(&root.join("lib/CoolGame/saves/s2.sav"), 30);

        let entry = GameEntry {
            title: "CoolGame".into(),
            pcgw_id: 1,
            install_folder: Some("CoolGame".into()),
            steam_id: None,
            gog_id: None,
            platforms: vec!["Steam".into()],
            zh_cn: None,
            paths: SaveLocations {
                // {{p|game}} = install_path（库根目录下的 CoolGame）
                win: vec!["{{p|game}}\\saves\\*.sav".into()],
                reg: vec![],
                mac: vec![],
                linux: vec![],
                win_kinds: vec!["saves".into()],
                reg_kinds: vec![],
                mac_kinds: vec![],
                linux_kinds: vec![],
            },
        };
        let env = DetectedEnvironment {
            platform: Platform::Windows,
            install_paths: vec![crate::detect::InstallRoot { path: root.join("lib").to_string_lossy().to_string(), platform: "Steam".into() }],
            steam_apps: HashMap::new(),
            steam_path: None,
            ubisoft_path: None,
            accounts: AccountIds::default(),
        };
        let home = root.clone();
        let scanned = scan_game(&entry, &env, &home, false);

        assert_eq!(scanned.resolved_files.len(), 2);
        assert_eq!(scanned.backup_size, 150);
        assert!(scanned.latest_modified_ms.is_some());
        assert!(scanned.install_path.is_some()); // install_folder 命中根目录
        assert!(scanned.resolved_files[0].final_template.contains("{{p|game}}"));
        let _ = fs::remove_dir_all(&root);
    }

    // ==================================================================
    // 卸载残留核验（find_installed_verified）
    // ==================================================================

    fn steam_root(path: &Path) -> crate::detect::InstallRoot {
        crate::detect::InstallRoot {
            path: path.to_string_lossy().to_string(),
            platform: "Steam".into(),
        }
    }

    #[test]
    fn leftover_empty_folder_is_not_installed() {
        // 回归：卸载后 steamapps 里残留的空目录树（只剩空子目录）曾被判为已安装
        let root = std::env::temp_dir().join(format!("gm-leftover-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("lib/GhostGame/_CommonRedist/vcredist")).unwrap();

        let roots = vec![steam_root(&root.join("lib"))];
        let apps = HashMap::new();
        let (path, platforms) =
            find_installed_verified("GhostGame", &roots, None, &apps);
        assert!(path.is_none(), "空壳目录树不应判为已安装: {path:?}");
        assert!(platforms.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_with_real_files_is_installed() {
        let root = std::env::temp_dir().join(format!("gm-real-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_file(&root.join("lib/RealGame/bin/game.exe"), 10);

        let roots = vec![steam_root(&root.join("lib"))];
        let apps = HashMap::new();
        let (path, platforms) = find_installed_verified("RealGame", &roots, None, &apps);
        assert!(path.is_some());
        assert_eq!(platforms, vec!["Steam".to_string()]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn steam_folder_without_appmanifest_is_not_installed() {
        // 回归（Lust Epidemic 案例）：已知 steam_id 的游戏，acf 已被 Steam 删除
        // （= 已卸载），即使残留文件夹里还有文件也不算已安装
        let root = std::env::temp_dir().join(format!("gm-noacf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_file(&root.join("lib/GhostGame/leftover.dll"), 10);

        let roots = vec![steam_root(&root.join("lib"))];
        let apps = HashMap::new(); // 无 appmanifest = Steam 已卸载
        let (path, platforms) =
            find_installed_verified("GhostGame", &roots, Some("12345"), &apps);
        assert!(path.is_none(), "acf 缺失的残留文件夹不应判为已安装: {path:?}");
        assert!(platforms.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn steam_appmanifest_is_authoritative_path() {
        // appmanifest 定位的安装目录即使与 install_folder 名称不同也以其为准
        let root = std::env::temp_dir().join(format!("gm-acf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_file(&root.join("lib/Actual Dir Name/game.exe"), 10);

        let roots = vec![steam_root(&root.join("lib"))];
        let mut apps = HashMap::new();
        apps.insert(
            "12345".to_string(),
            root.join("lib/Actual Dir Name").to_string_lossy().to_string(),
        );
        let (path, platforms) =
            find_installed_verified("Whatever", &roots, Some("12345"), &apps);
        assert!(path.unwrap().ends_with("Actual Dir Name"));
        assert_eq!(platforms, vec!["Steam".to_string()]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn uninstalled_game_saves_inside_leftover_folder_still_found() {
        // 回归（Lust Epidemic 案例的另一半）：游戏已卸载（acf 没了）→ 不算已安装；
        // 但存档留在安装目录里（{{p|game}} 模板）→ 必须仍能扫到（残留存档可备份）
        let root = std::env::temp_dir().join(format!("gm-leftsave-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_file(&root.join("lib/GhostGame/saves/s1.sav"), 64);

        let entry = GameEntry {
            title: "GhostGame".into(),
            pcgw_id: 7,
            install_folder: Some("GhostGame".into()),
            steam_id: Some("12345".into()), // 已知 id 但无 acf = 已卸载
            gog_id: None,
            platforms: vec!["Steam".into()],
            zh_cn: None,
            paths: SaveLocations {
                win: vec!["{{p|game}}\\saves\\*.sav".into()],
                reg: vec![],
                mac: vec![],
                linux: vec![],
                win_kinds: vec!["saves".into()],
                reg_kinds: vec![],
                mac_kinds: vec![],
                linux_kinds: vec![],
            },
        };
        let env = DetectedEnvironment {
            platform: Platform::Windows,
            install_paths: vec![steam_root(&root.join("lib"))],
            steam_apps: HashMap::new(),
            steam_path: None,
            ubisoft_path: None,
            accounts: AccountIds::default(),
        };
        let home = root.clone();
        let scanned = scan_game(&entry, &env, &home, false);

        assert!(scanned.install_path.is_none(), "已卸载不应判安装");
        assert!(scanned.installed_platforms.is_empty());
        assert_eq!(scanned.resolved_files.len(), 1, "残留文件夹里的存档必须扫到");
        assert_eq!(scanned.backup_size, 64);
        let _ = fs::remove_dir_all(&root);
    }
}
