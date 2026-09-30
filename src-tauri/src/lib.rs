mod auto_backup;

use gm_core::settings::AppSettings;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;

/// 应用全局状态：设置 + 数据库位置（tauri State 管理）
pub struct AppState {
    pub settings: Mutex<AppSettings>,
    pub db_path: PathBuf,
    pub settings_path: PathBuf,
    pub app_data_dir: PathBuf,
    pub custom_games_path: PathBuf,
}

impl AppState {
    fn backups_root(&self) -> PathBuf {
        let root = self
            .settings
            .lock()
            .unwrap()
            .backup_path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.app_data_dir.join("Backups"));
        let _ = std::fs::create_dir_all(&root);
        root
    }

    fn max_backups(&self) -> u32 {
        self.settings.lock().unwrap().max_backups
    }

    fn compress(&self) -> bool {
        self.settings.lock().unwrap().compress_backups
    }

    fn persist(&self, settings: &AppSettings) -> Result<(), String> {
        settings.save(&self.settings_path)?;
        *self.settings.lock().unwrap() = settings.clone();
        Ok(())
    }
}

/// 应用信息
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub core_version: String,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        core_version: gm_core::version().to_string(),
    }
}

// ======================================================================
// 定位与工具
// ======================================================================

/// 定位数据库文件：打包环境用资源目录，开发环境回退到 src-tauri/resources
fn database_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    if let Ok(dir) = app.path().resource_dir() {
        for candidate in [dir.join("resources/database.db"), dir.join("database.db")] {
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/database.db");
    if dev_path.exists() {
        return Ok(dev_path);
    }
    Err("找不到数据库文件（resources/database.db）".to_string())
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

// ======================================================================
// 设置命令
// ======================================================================

#[tauri::command]
fn list_modules() -> Vec<gm_core::module::ModuleInfo> {
    gm_core::module::modules()
}

#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    backup_path: Option<String>,
    max_backups: Option<u32>,
    compress_backups: Option<bool>,
    auto_backup_enabled: Option<bool>,
    close_to_tray: Option<bool>,
) -> Result<AppSettings, String> {
    let was_enabled = state.settings.lock().unwrap().auto_backup_enabled;
    let mut settings = state.settings.lock().unwrap().clone();
    if let Some(path) = backup_path {
        settings.backup_path = if path.trim().is_empty() { None } else { Some(path) };
    }
    if let Some(max) = max_backups {
        settings.max_backups = max.clamp(1, 100);
    }
    if let Some(compress) = compress_backups {
        settings.compress_backups = compress;
    }
    if let Some(enabled) = auto_backup_enabled {
        settings.auto_backup_enabled = enabled;
    }
    if let Some(tray) = close_to_tray {
        settings.close_to_tray = tray;
    }
    state.persist(&settings)?;
    // 全局自动备份开关变化 → 立即应用（开启：扫描并挂线程；关闭：全部停止）
    if settings.auto_backup_enabled != was_enabled {
        let app2 = app.clone();
        std::thread::spawn(move || {
            if let Err(e) = refresh_auto_backup_blocking(&app2) {
                eprintln!("[auto-backup] 应用全局开关失败: {e}");
            }
        });
    }
    Ok(settings)
}

#[tauri::command]
fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .blocking_pick_folder()
        .map(|p| p.to_string())
}

#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable().map_err(|e| e.to_string())?;
    } else {
        autolaunch.disable().map_err(|e| e.to_string())?;
    }
    Ok(autolaunch.is_enabled().unwrap_or(false))
}

// ======================================================================
// 游戏数据库命令
// ======================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamesPage {
    pub items: Vec<gm_core::database::GameEntry>,
    pub total: i64,
}

#[tauri::command]
async fn list_games(
    state: tauri::State<'_, AppState>,
    search: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<GamesPage, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = gm_core::database::Database::open_readonly(&db_path)?;
        let limit = limit.unwrap_or(200).min(1000);
        let offset = offset.unwrap_or(0);
        let items = db.list_games(search.as_deref(), limit, offset)?;
        let total = db.total_games()?;
        Ok(GamesPage { items, total })
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
}

// ======================================================================
// 环境探测与扫描命令
// ======================================================================

#[tauri::command]
fn detect_environment() -> gm_core::detect::DetectedEnvironment {
    gm_core::detect::detect()
}

#[tauri::command]
async fn scan_installed_games(
    state: tauri::State<'_, AppState>,
    all_accounts: Option<bool>,
) -> Result<gm_core::scanner::ScanOutput, String> {
    let db_path = state.db_path.clone();
    let customs_path = state.custom_games_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = gm_core::database::Database::open_readonly(&db_path)?;
        let env = gm_core::detect::detect();
        let home = home_dir();
        let customs = gm_core::custom::load(&customs_path);
        gm_core::scanner::quick_scan(&db, &env, &home, all_accounts.unwrap_or(false), &customs)
    })
    .await
    .map_err(|e| format!("扫描任务失败: {e}"))?
}

#[tauri::command]
async fn scan_deep_games(
    state: tauri::State<'_, AppState>,
    all_accounts: Option<bool>,
) -> Result<gm_core::scanner::ScanOutput, String> {
    let db_path = state.db_path.clone();
    let customs_path = state.custom_games_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = gm_core::database::Database::open_readonly(&db_path)?;
        let env = gm_core::detect::detect();
        let home = home_dir();
        let customs = gm_core::custom::load(&customs_path);
        gm_core::scanner::deep_scan(&db, &env, &home, all_accounts.unwrap_or(false), &customs)
    })
    .await
    .map_err(|e| format!("深度扫描任务失败: {e}"))?
}

// ======================================================================
// 自定义游戏命令
// ======================================================================

#[tauri::command]
fn list_custom_games(state: tauri::State<AppState>) -> Vec<gm_core::custom::CustomGame> {
    gm_core::custom::load(&state.custom_games_path)
}

#[tauri::command]
fn save_custom_games(
    state: tauri::State<AppState>,
    games: Vec<gm_core::custom::CustomGame>,
) -> Result<Vec<gm_core::custom::CustomGame>, String> {
    let mut games = games;
    gm_core::custom::assign_ids(&mut games);
    // 清洗：去掉空名字/空路径的条目
    games.retain(|g| !g.name.trim().is_empty() && !g.paths.is_empty());
    gm_core::custom::save(&state.custom_games_path, &games)?;
    Ok(games)
}

#[tauri::command]
fn pick_file(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .blocking_pick_file()
        .map(|p| p.to_string())
}

#[tauri::command]
async fn game_detail(
    state: tauri::State<'_, AppState>,
    wiki_page_id: i64,
    all_accounts: Option<bool>,
) -> Result<Option<gm_core::scanner::ScannedGame>, String> {
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = gm_core::database::Database::open_readonly(&db_path)?;
        let Some(entry) = db.get_game(wiki_page_id)? else {
            return Ok(None);
        };
        let env = gm_core::detect::detect();
        let home = home_dir();
        Ok(Some(gm_core::scanner::scan_game(
            &entry,
            &env,
            &home,
            all_accounts.unwrap_or(false),
        )))
    })
    .await
    .map_err(|e| format!("解析任务失败: {e}"))?
}

// ======================================================================
// 备份与恢复命令
// ======================================================================

#[tauri::command]
async fn backup_game(
    state: tauri::State<'_, AppState>,
    wiki_page_id: i64,
    all_accounts: Option<bool>,
) -> Result<String, String> {
    let db_path = state.db_path.clone();
    let root = state.backups_root();
    let max = state.max_backups();
    let compress = state.compress();
    tauri::async_runtime::spawn_blocking(move || {
        let db = gm_core::database::Database::open_readonly(&db_path)?;
        let entry = db
            .get_game(wiki_page_id)?
            .ok_or_else(|| format!("数据库里没有 wiki_page_id = {wiki_page_id} 的游戏"))?;
        let env = gm_core::detect::detect();
        let home = home_dir();
        let scanned =
            gm_core::scanner::scan_game(&entry, &env, &home, all_accounts.unwrap_or(false));
        if scanned.resolved_files.is_empty() {
            return Err("没有找到可备份的存档文件".to_string());
        }
        gm_core::backup::backup_game(&entry, &scanned.resolved_files, &root, max, compress)
    })
    .await
    .map_err(|e| format!("备份任务失败: {e}"))?
}

#[tauri::command]
async fn list_backups(
    state: tauri::State<'_, AppState>,
    wiki_page_id: i64,
) -> Result<Option<gm_core::backup::GameBackups>, String> {
    let root = state.backups_root();
    tauri::async_runtime::spawn_blocking(move || {
        gm_core::backup::list_backups(&root, wiki_page_id)
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
}

#[tauri::command]
async fn check_restore_conflict(
    state: tauri::State<'_, AppState>,
    wiki_page_id: i64,
    date: String,
) -> Result<gm_core::restore::ConflictInfo, String> {
    let root = state.backups_root();
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = gm_core::database::Database::open_readonly(&db_path)?;
        let entry = db
            .get_game(wiki_page_id)?
            .ok_or_else(|| format!("数据库里没有 wiki_page_id = {wiki_page_id} 的游戏"))?;
        let env = gm_core::detect::detect();
        let home = home_dir();
        let game = gm_core::backup::list_backups(&root, wiki_page_id)?
            .ok_or_else(|| "该游戏还没有任何备份".to_string())?;
        gm_core::restore::check_restore_conflict(&root, &game, &date, &env, &home).map(|mut info| {
            // 目标不存在时视为无冲突（本地没有存档就谈不上覆盖）
            if entry
                .install_folder
                .as_deref()
                .and_then(|f| gm_core::scanner::find_install_path(f, &env.install_paths))
                .is_none()
                && info.local_ms.is_none()
            {
                info.conflict = false;
            }
            info
        })
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
}

#[tauri::command]
async fn restore_backup(
    state: tauri::State<'_, AppState>,
    wiki_page_id: i64,
    date: String,
    force: Option<bool>,
) -> Result<gm_core::restore::RestoreResult, String> {
    let root = state.backups_root();
    tauri::async_runtime::spawn_blocking(move || {
        let env = gm_core::detect::detect();
        let home = home_dir();
        let game = gm_core::backup::list_backups(&root, wiki_page_id)?
            .ok_or_else(|| "该游戏还没有任何备份".to_string())?;
        gm_core::restore::restore_backup(&root, &game, &date, &env, &home, force.unwrap_or(false))
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
}

#[tauri::command]
async fn list_all_backups(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<gm_core::backup::GameBackups>, String> {
    let root = state.backups_root();
    tauri::async_runtime::spawn_blocking(move || gm_core::backup::list_all_backups(&root))
        .await
        .map_err(|e| format!("任务失败: {e}"))?
}

#[tauri::command]
async fn delete_backup(
    state: tauri::State<'_, AppState>,
    wiki_page_id: i64,
    date: String,
) -> Result<(), String> {
    let root = state.backups_root();
    tauri::async_runtime::spawn_blocking(move || {
        gm_core::backup::delete_backup(&root, wiki_page_id, &date)
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
}

#[tauri::command]
async fn set_backup_permanent(
    state: tauri::State<'_, AppState>,
    wiki_page_id: i64,
    date: String,
    permanent: bool,
) -> Result<(), String> {
    let root = state.backups_root();
    tauri::async_runtime::spawn_blocking(move || {
        gm_core::backup::set_backup_permanent(&root, wiki_page_id, &date, permanent)
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
}

// ======================================================================
// 自动备份命令
// ======================================================================
// 自动备份（全局开关）
// ======================================================================

/// 依据全局开关同步自动备份：
/// 开启 → 为所有已安装游戏挂"存档变化即备份" + "游戏退出时备份"两条线程；
/// 关闭 → 全部停止。返回本轮新启动的任务数。
fn refresh_auto_backup_blocking(app: &tauri::AppHandle) -> Result<usize, String> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let settings = state.settings.lock().unwrap().clone();
    if !settings.auto_backup_enabled {
        auto_backup::manager().lock().unwrap().stop_all();
        return Ok(0);
    }
    let db_path = state.db_path.clone();
    let root = state.backups_root();
    let max = state.max_backups();
    let compress = state.compress();
    let customs_path = state.custom_games_path.clone();
    let output = gm_core::scanner::quick_scan(
        &gm_core::database::Database::open_readonly(&db_path)?,
        &gm_core::detect::detect(),
        &home_dir(),
        false,
        &gm_core::custom::load(&customs_path),
    )?;
    let keep: Vec<i64> = output.games.iter().map(|g| g.entry.wiki_page_id).collect();
    let mut started = 0usize;
    {
        let mut manager = auto_backup::manager().lock().unwrap();
        manager.retain(&keep); // 停止已不在列表中的游戏（卸载/移除）
        for game in &output.games {
            let id = game.entry.wiki_page_id;
            if manager.is_active(id) {
                continue; // 已在监测，不重复挂线程
            }
            let paths: Vec<PathBuf> = game
                .resolved_files
                .iter()
                .map(|r| PathBuf::from(&r.resolved))
                .collect();
            manager.start_watcher(id, paths, db_path.clone(), root.clone(), max, compress);
            started += 1;
            if id >= 0 {
                if let Some(ip) = &game.install_path {
                    let exes = auto_backup::collect_exe_names(std::path::Path::new(ip));
                    manager.start_exit(id, exes, db_path.clone(), root.clone(), max, compress);
                    started += 1;
                }
            }
        }
    }
    Ok(started)
}

#[tauri::command]
async fn refresh_auto_backup(app: tauri::AppHandle) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || refresh_auto_backup_blocking(&app))
        .await
        .map_err(|e| format!("任务失败: {e}"))?
}

// 批量操作 / 备份迁移 / 打开目录
// ======================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub wiki_page_id: i64,
    pub title: String,
    pub ok: bool,
    pub message: Option<String>,
}

#[tauri::command]
async fn batch_backup(
    state: tauri::State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<Vec<BatchResult>, String> {
    let db_path = state.db_path.clone();
    let root = state.backups_root();
    let max = state.max_backups();
    let compress = state.compress();
    tauri::async_runtime::spawn_blocking(move || {
        let db = gm_core::database::Database::open_readonly(&db_path)?;
        let env = gm_core::detect::detect();
        let home = home_dir();
        let mut out = Vec::new();
        for id in ids {
            let result = (|| -> Result<String, String> {
                let entry = db
                    .get_game(id)?
                    .ok_or_else(|| format!("数据库里没有 wiki_page_id = {id} 的游戏"))?;
                let scanned = gm_core::scanner::scan_game(&entry, &env, &home, false);
                if scanned.resolved_files.is_empty() {
                    return Err("没有找到可备份的存档文件".to_string());
                }
                gm_core::backup::backup_game(&entry, &scanned.resolved_files, &root, max, compress)
            })();
            // 失败时标题从库里补（备份成功时 result 里已有路径，标题单独查）
            let title = db
                .get_game(id)
                .ok()
                .flatten()
                .map(|e| e.title)
                .unwrap_or_else(|| id.to_string());
            out.push(BatchResult {
                wiki_page_id: id,
                title,
                ok: result.is_ok(),
                message: result.err(),
            });
        }
        Ok(out)
    })
    .await
    .map_err(|e| format!("批量任务失败: {e}"))?
}

#[tauri::command]
async fn batch_restore_latest(
    state: tauri::State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<Vec<BatchResult>, String> {
    let root = state.backups_root();
    let db_path = state.db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = gm_core::database::Database::open_readonly(&db_path)?;
        let env = gm_core::detect::detect();
        let home = home_dir();
        let mut out = Vec::new();
        for id in ids {
            let title = db
                .get_game(id)
                .ok()
                .flatten()
                .map(|e| e.title)
                .unwrap_or_else(|| id.to_string());
            let result = (|| -> Result<String, String> {
                let game = gm_core::backup::list_backups(&root, id)?
                    .ok_or_else(|| "没有任何备份".to_string())?;
                let latest = &game.backups[0];
                let conflict =
                    gm_core::restore::check_restore_conflict(&root, &game, &latest.date, &env, &home)?;
                if conflict.conflict {
                    return Err("本地存档较新，已跳过（请单独恢复并确认）".to_string());
                }
                gm_core::restore::restore_backup(&root, &game, &latest.date, &env, &home, false)
                    .map(|r| format!("恢复 {} 个路径", r.restored_paths))
            })();
            out.push(BatchResult {
                wiki_page_id: id,
                title,
                ok: result.is_ok(),
                message: result.err(),
            });
        }
        Ok(out)
    })
    .await
    .map_err(|e| format!("批量任务失败: {e}"))?
}

#[tauri::command]
async fn migrate_backups(
    state: tauri::State<'_, AppState>,
    new_path: String,
) -> Result<usize, String> {
    let old_root = state.backups_root();
    let new_root = std::path::PathBuf::from(&new_path);
    let moved = tauri::async_runtime::spawn_blocking(move || -> Result<usize, String> {
        if old_root == new_root {
            return Ok(0);
        }
        std::fs::create_dir_all(&new_root).map_err(|e| format!("创建新目录失败: {e}"))?;
        let mut moved = 0usize;
        for entry in std::fs::read_dir(&old_root).map_err(|e| e.to_string())?.flatten() {
            if !entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                continue;
            }
            let dest = new_root.join(entry.file_name());
            // 同盘直接改名；跨盘退化为复制 + 删除
            if std::fs::rename(entry.path(), &dest).is_err() {
                gm_core::backup::copy_dir_recursive(&entry.path(), &dest)?;
                std::fs::remove_dir_all(entry.path()).map_err(|e| e.to_string())?;
            }
            moved += 1;
        }
        Ok(moved)
    })
    .await
    .map_err(|e| format!("迁移任务失败: {e}"))??;

    // 迁移成功后才切换设置里的备份位置
    let mut settings = state.settings.lock().unwrap().clone();
    settings.backup_path = Some(new_path);
    state.persist(&settings)?;
    Ok(moved)
}

#[tauri::command]
fn open_game_backup_folder(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    wiki_page_id: i64,
) -> Result<(), String> {
    let dir = state.backups_root().join(wiki_page_id.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// 打开存档所在文件夹：路径是目录就开它本身，是文件就开父目录
#[tauri::command]
fn open_save_folder(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("路径不存在: {path}"));
    }
    let dir = if p.is_dir() {
        p
    } else {
        p.parent()
            .map(std::path::PathBuf::from)
            .ok_or_else(|| "无法定位父目录".to_string())?
    };
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        // 更新器在接入自己的服务器后启用（届时填 plugins.updater 配置）
        .setup(|app| {
            use tauri::Manager;
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法定位应用数据目录: {e}"))?;
            let settings_path = app_data_dir.join("settings.json");
            let settings = AppSettings::load(&settings_path);
            let db_path = database_path(app.handle()).unwrap_or_else(|_| PathBuf::new());

            // 全局自动备份开关：开启时延迟 1.2 秒执行刷新（等窗口就绪，避免阻塞启动）
            if settings.auto_backup_enabled && db_path.exists() {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                    if let Err(e) = refresh_auto_backup_blocking(&handle) {
                        eprintln!("[auto-backup] 启动恢复失败: {e}");
                    }
                });
            }

            app.manage(AppState {
                settings: Mutex::new(settings),
                db_path,
                settings_path,
                custom_games_path: app_data_dir.join("custom_games.json"),
                app_data_dir: app_data_dir.clone(),
            });

            // 系统托盘：显示主窗口 / 打开备份文件夹 / 退出
            use tauri::{
                menu::{Menu, MenuItem},
                tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
            };
            let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let open_backups =
                MenuItem::with_id(app, "open_backups", "打开备份文件夹", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &open_backups, &quit])?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().expect("missing app icon").clone())
                .tooltip("Game Manager")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                    "open_backups" => {
                        let root = app.state::<AppState>().backups_root();
                        let _ = std::fs::create_dir_all(&root);
                        use tauri_plugin_opener::OpenerExt;
                        let _ = app.opener().open_path(root.to_string_lossy(), None::<&str>);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        // 关闭主窗口：按设置进入托盘（默认）或直接退出；真正退出也可走托盘菜单
        .on_window_event(|window, event| {
            use tauri::Manager;
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let to_tray = window
                        .app_handle()
                        .try_state::<AppState>()
                        .map(|s| s.settings.lock().unwrap().close_to_tray)
                        .unwrap_or(true);
                    if to_tray {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_modules,
            list_games,
            detect_environment,
            scan_installed_games,
            scan_deep_games,
            game_detail,
            list_custom_games,
            save_custom_games,
            pick_file,
            backup_game,
            list_backups,
            list_all_backups,
            check_restore_conflict,
            restore_backup,
            delete_backup,
            set_backup_permanent,
            batch_backup,
            batch_restore_latest,
            migrate_backups,
            open_game_backup_folder,
            open_save_folder,
            get_settings,
            save_settings,
            pick_folder,
            get_autostart,
            set_autostart,
            refresh_auto_backup
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
