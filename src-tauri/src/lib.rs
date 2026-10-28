mod auto_backup;
mod game_cover;
mod steamgriddb;
mod webdav;

use gm_core::settings::AppSettings;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 应用全局状态：设置 + 数据库位置（tauri State 管理）
pub struct AppState {
    pub settings: Mutex<AppSettings>,
    /// 当前生效的存档数据库路径：内置资源，或用户更新版（app_data/database.db）
    pub db_path: Mutex<PathBuf>,
    pub settings_path: PathBuf,
    pub app_data_dir: PathBuf,
    pub custom_games_path: PathBuf,
    /// 修改器目录注册表（启动装载；目录在线更新后整体替换）
    pub trainers: std::sync::RwLock<gm_core::trainers::TrainerRegistry>,
    /// 修改器中文名索引（来自游戏库 title→zh_CN，启动构建；数据库更新后重建）
    pub trainer_zh: std::sync::RwLock<gm_core::trainers::ZhNameIndex>,
    /// 正在运行的修改器下载取消标记（trainerId → flag）
    pub trainer_downloads: Mutex<HashMap<String, Arc<AtomicBool>>>,
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

/// 应用更新检查结果（匿名查 GitHub Release，无需令牌）
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub has_update: bool,
    pub release_url: String,
}

/// major.minor.patch 数值比较（tag 形如 v0.3.0）
fn version_newer(tag: &str, current: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .map(|p| p.trim().parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(tag), parse(current));
    for i in 0..3 {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

#[tauri::command]
async fn check_app_update() -> Result<AppUpdateInfo, String> {
    const RELEASES_API: &str =
        "https://api.github.com/repos/LinkJudge/Game-Manage-Center/releases/latest";
    const RELEASE_PAGE: &str =
        "https://github.com/LinkJudge/Game-Manage-Center/releases/latest";
    let current = env!("CARGO_PKG_VERSION").to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let client = download_client()?;
        let json: serde_json::Value = client
            .get(RELEASES_API)
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| format!("无法连接 GitHub: {e}"))?
            .error_for_status()
            .map_err(|e| format!("查询发布信息被拒绝（{}）", e.status().unwrap_or_default()))?
            .text()
            .map_err(|e| format!("读取发布信息失败: {e}"))
            .and_then(|text| serde_json::from_str(&text).map_err(|e| format!("解析发布信息失败: {e}")))?;
        let tag = json
            .get("tag_name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_start_matches('v').to_string());
        let has_update = tag.as_deref().map(|t| version_newer(t, &current)).unwrap_or(false);
        Ok(AppUpdateInfo {
            current_version: current,
            latest_version: tag,
            has_update,
            release_url: RELEASE_PAGE.to_string(),
        })
    })
    .await
    .map_err(|e| format!("更新检查任务失败: {e}"))?
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

/// 定位修改器目录资源（resources/trainers，每来源一份 catalog JSON）
fn trainers_dir(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    if let Ok(dir) = app.path().resource_dir() {
        for candidate in [dir.join("resources/trainers"), dir.join("trainers")] {
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/trainers")
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

// ======================================================================
// 修改器命令（目录检索与游戏匹配；下载/安装后续接入）
// ======================================================================

/// 来源元信息摘要（list_trainer_sources 返回）
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerSourceSummary {
    pub id: String,
    pub title: String,
    pub homepage: String,
    pub count: usize,
}

#[tauri::command]
fn list_trainer_sources(state: tauri::State<AppState>) -> Vec<TrainerSourceSummary> {
    state
        .trainers
        .read()
        .unwrap()
        .source_infos()
        .into_iter()
        .map(|(info, count)| TrainerSourceSummary {
            id: info.id,
            title: info.title,
            homepage: info.homepage,
            count,
        })
        .collect()
}

#[tauri::command]
fn search_trainers(
    state: tauri::State<AppState>,
    query: String,
    limit: Option<u32>,
) -> Vec<gm_core::trainers::TrainerHit> {
    state
        .trainers
        .read()
        .unwrap()
        .search(
            &query,
            &state.trainer_zh.read().unwrap(),
            limit.unwrap_or(50).clamp(1, 200) as usize,
        )
}

/// 游戏匹配查询输入：前端把扫描到的游戏（标题 + 可选中文名）传回来
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerGameQuery {
    pub title: String,
    #[serde(default)]
    pub zh_name: Option<String>,
}

#[tauri::command]
fn match_trainers(
    state: tauri::State<AppState>,
    games: Vec<TrainerGameQuery>,
) -> Vec<gm_core::trainers::GameTrainerMatch> {
    let pairs: Vec<(String, Option<String>)> =
        games.into_iter().map(|g| (g.title, g.zh_name)).collect();
    state
        .trainers
        .read()
        .unwrap()
        .match_games(&state.trainer_zh.read().unwrap(), &pairs)
}

// ======================================================================
// 修改器下载与安装
// ======================================================================

/// 修改器安装根目录：设置 trainerPath 优先，默认应用数据目录/Trainers
fn trainers_root(state: &AppState) -> PathBuf {
    let root = state
        .settings
        .lock()
        .unwrap()
        .trainer_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| state.app_data_dir.join("Trainers"));
    let _ = std::fs::create_dir_all(&root);
    root
}

/// 7z.exe 位置（随应用资源分发；开发环境回退 src-tauri/resources/bin）
fn sevenz_path(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    if let Ok(dir) = app.path().resource_dir() {
        for candidate in [dir.join("resources/bin/7z.exe"), dir.join("bin/7z.exe")] {
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/bin/7z.exe")
}

/// 下载进度事件载荷（stage: download / extract / done）
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrainerProgress {
    trainer_id: String,
    stage: String,
    downloaded: u64,
    total: u64,
}

const DOWNLOAD_CANCELLED: &str = "__TRAINER_DOWNLOAD_CANCELLED__";

fn cancelled_error() -> String {
    DOWNLOAD_CANCELLED.to_string()
}

fn ensure_not_cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err(cancelled_error())
    } else {
        Ok(())
    }
}

#[tauri::command]
fn list_installed_trainers(
    state: tauri::State<AppState>,
) -> Vec<gm_core::trainers::install::InstalledTrainer> {
    gm_core::trainers::install::scan_installed(&trainers_root(&state))
}

/// 下载客户端：站点要求 UA + Referer；代理走环境变量（Clash 场景）
fn download_client() -> Result<reqwest::blocking::Client, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36",
        )
        .connect_timeout(std::time::Duration::from_secs(15));
    if let Ok(proxy) = std::env::var("HTTPS_PROXY").or_else(|_| std::env::var("https_proxy")) {
        builder = builder.proxy(reqwest::Proxy::all(&proxy).map_err(|e| format!("代理配置失败: {e}"))?);
    }
    builder.build().map_err(|e| e.to_string())
}

/// 由条目 id + 下载地址组装安装元数据（条目与附件都在目录里，前端只传这两个）
fn resolve_install_meta(
    state: &AppState,
    trainer_id: &str,
    download_url: &str,
) -> Result<gm_core::trainers::install::TrainerInstallMeta, String> {
    let registry = state.trainers.read().unwrap();
    let Some((_source_id, entry)) = registry.find_entry(trainer_id) else {
        return Err(format!("目录里找不到修改器：{trainer_id}"));
    };
    let Some(dl) = entry.downloads.iter().find(|d| d.url == download_url) else {
        return Err("目录里找不到该下载地址（请刷新搜索）".to_string());
    };
    if dl.external {
        return Err("该来源只提供官网/网盘下载页，请使用「官网获取」手动下载".to_string());
    }
    Ok(gm_core::trainers::install::TrainerInstallMeta {
        trainer_id: entry.id.clone(),
        source_id: _source_id.to_string(),
        game_name: entry.game_name.clone(),
        zh_name: entry.zh_name.clone(),
        file: dl.file.clone(),
        download_url: dl.url.clone(),
        page_url: entry.page_url.clone(),
        date_added: dl.date_added.clone(),
        game_version: dl.game_version.clone(),
        options_count: dl.options_count,
        size_bytes: dl.size_bytes,
    })
}

/// 单流下载（Range 不可用或文件较小时的回退路径）。返回记录用的大小
fn download_single_stream(
    client: &reqwest::blocking::Client,
    url: &reqwest::Url,
    referer: &str,
    tmp_file: &std::path::Path,
    cancel: &AtomicBool,
    emit: &dyn Fn(&str, u64, u64),
) -> Result<u64, String> {
    use std::io::{Read, Write};
    ensure_not_cancelled(cancel)?;
    let mut resp = client
        .get(url.clone())
        .header("Referer", referer)
        .send()
        .map_err(|e| format!("下载请求失败: {e}"))?
        .error_for_status()
        .map_err(|e| {
            format!(
                "下载被拒绝（{}）。站点可能开启风控，稍后再试",
                e.status().unwrap_or_default()
            )
        })?;
    ensure_not_cancelled(cancel)?;
    let total = resp.content_length().unwrap_or(0);
    let mut file =
        std::fs::File::create(tmp_file).map_err(|e| format!("创建临时文件失败: {e}"))?;
    let mut buf = [0u8; 64 * 1024];
    let mut done = 0u64;
    let mut last_emit = std::time::Instant::now();
    loop {
        ensure_not_cancelled(cancel)?;
        let n = resp.read(&mut buf).map_err(|e| format!("下载中断: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("写入失败: {e}"))?;
        done += n as u64;
        if last_emit.elapsed() >= std::time::Duration::from_millis(150) {
            emit("download", done, total);
            last_emit = std::time::Instant::now();
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    emit("download", done, total);
    Ok(total.max(done))
}

/// 多连接分段并行下载：每段独立连接（各付一次连接/重定向开销），单连接被
/// CDN 限速时总速度接近分段数倍数。任一分段失败即整体失败（staging 由外层清理）
fn download_ranged_parallel(
    final_url: &reqwest::Url,
    referer: &str,
    tmp_file: &std::path::Path,
    total: u64,
    segments: u64,
    cancel: &AtomicBool,
    emit: &dyn Fn(&str, u64, u64),
) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, Mutex};

    ensure_not_cancelled(cancel)?;
    // 预分配目标文件，各分段按偏移写入
    {
        let f = std::fs::File::create(tmp_file).map_err(|e| format!("创建临时文件失败: {e}"))?;
        f.set_len(total).map_err(|e| format!("预分配失败: {e}"))?;
    }
    let progress = Arc::new(AtomicU64::new(0));
    let first_err: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let seg_size = total.div_ceil(segments);
    std::thread::scope(|s| {
        for i in 0..segments {
            let start = i * seg_size;
            if start >= total {
                break;
            }
            let end = (start + seg_size - 1).min(total - 1);
            let progress = Arc::clone(&progress);
            let first_err = Arc::clone(&first_err);
            s.spawn(move || {
                let run = || -> Result<(), String> {
                    ensure_not_cancelled(cancel)?;
                    let client = download_client()?;
                    let mut resp = client
                        .get(final_url.clone())
                        .header("Referer", referer)
                        .header("Range", format!("bytes={start}-{end}"))
                        .send()
                        .map_err(|e| format!("分段 {i} 请求失败: {e}"))?;
                    if resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                        return Err(format!("分段 {i} 被拒绝（{}）", resp.status()));
                    }
                    let mut file = std::fs::OpenOptions::new()
                        .write(true)
                        .open(tmp_file)
                        .map_err(|e| format!("分段 {i} 打开临时文件失败: {e}"))?;
                    file.seek(SeekFrom::Start(start)).map_err(|e| e.to_string())?;
                    let mut buf = [0u8; 64 * 1024];
                    loop {
                        ensure_not_cancelled(cancel)?;
                        let n =
                            resp.read(&mut buf).map_err(|e| format!("分段 {i} 下载中断: {e}"))?;
                        if n == 0 {
                            break;
                        }
                        file.write_all(&buf[..n])
                            .map_err(|e| format!("分段 {i} 写入失败: {e}"))?;
                        progress.fetch_add(n as u64, Ordering::Relaxed);
                    }
                    Ok(())
                };
                if let Err(e) = run() {
                    let mut slot = first_err.lock().unwrap();
                    if slot.is_none() {
                        *slot = Some(e);
                    }
                }
            });
        }
        // 进度泵：分段线程各自跑，这里按 150ms 节拍发聚合进度
        loop {
            std::thread::sleep(std::time::Duration::from_millis(150));
            if cancel.load(Ordering::Relaxed) {
                let mut slot = first_err.lock().unwrap();
                if slot.is_none() {
                    *slot = Some(cancelled_error());
                }
                break;
            }
            let done = progress.load(Ordering::Relaxed);
            emit("download", done.min(total), total);
            if done >= total || first_err.lock().unwrap().is_some() {
                break;
            }
        }
    });
    if let Some(e) = first_err.lock().unwrap().take() {
        return Err(e);
    }
    ensure_not_cancelled(cancel)?;
    emit("download", total, total);
    Ok(())
}

/// 阻塞式下载 + 安装（进度事件照发）；调用方负责放在 spawn_blocking 里。
/// 7z 路径由命令层解析后显式传入（打包环境位于资源目录）。
fn download_and_install_blocking(
    app: &tauri::AppHandle,
    root: &std::path::Path,
    meta: &gm_core::trainers::install::TrainerInstallMeta,
    sevenz: &std::path::Path,
    cancel: &AtomicBool,
) -> Result<gm_core::trainers::install::InstalledTrainer, String> {
    use tauri::Emitter;

    let emit = |stage: &str, done: u64, total: u64| {
        let _ = app.emit(
            "trainer-download-progress",
            TrainerProgress {
                trainer_id: meta.trainer_id.clone(),
                stage: stage.to_string(),
                downloaded: done,
                total,
            },
        );
    };

    // 防重复（同一下载地址只装一次）
    if gm_core::trainers::install::installed_url_index(root).contains(&meta.download_url) {
        return Err("该修改器已安装".to_string());
    }
    ensure_not_cancelled(cancel)?;

    // 立即发一帧"连接中"进度：前端收到事件就把按钮换成进度条，
    // 否则要等重定向链走完（站点慢时数秒）界面才有反应
    emit("download", 0, 0);

    // 下载到临时目录（staging，失败整体清理）
    let tmp_dir = root.join(format!(".download-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let result = (|| -> Result<gm_core::trainers::install::InstalledTrainer, String> {
        let client = download_client()?;
        let tmp_file = tmp_dir.join("package.bin");
        ensure_not_cancelled(cancel)?;

        // 探测：Range 请求随重定向拿到最终地址与总大小（CDN 支持 206 分段）
        let probe = client
            .get(&meta.download_url)
            .header("Referer", &meta.page_url)
            .header("Range", "bytes=0-0")
            .send()
            .map_err(|e| format!("下载请求失败: {e}"))?;
        ensure_not_cancelled(cancel)?;
        let ranged_total: Option<u64> = if probe.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            probe
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.rsplit('/').next())
                .and_then(|s| s.trim().parse::<u64>().ok())
        } else {
            None
        };
        let final_url = probe.url().clone();
        drop(probe);

        let total = match ranged_total {
            // 支持分段且足够大：多连接并行拉取（单连接被限速时显著提速）
            Some(t) if t >= 256 * 1024 => {
                let segments = if t >= 512 * 1024 { 4u64 } else { 2u64 };
                download_ranged_parallel(
                    &final_url,
                    &meta.page_url,
                    &tmp_file,
                    t,
                    segments,
                    cancel,
                    &emit,
                )?;
                t
            }
            // 其余回退单流下载
            _ => download_single_stream(&client, &final_url, &meta.page_url, &tmp_file, cancel, &emit)?,
        };

        ensure_not_cancelled(cancel)?;
        // 安装（解压/落位/写 manifest）
        emit("extract", 0, total);
        let outcome =
            gm_core::trainers::install::install_package(&tmp_file, meta, root, sevenz)?;
        emit("done", total, total);

        Ok(gm_core::trainers::install::InstalledTrainer {
            manifest: outcome.manifest,
            folder: outcome.folder.to_string_lossy().to_string(),
        })
    })();

    let _ = std::fs::remove_dir_all(&tmp_dir);
    result
}

/// 下载并安装一个修改器包（流式下载 + 进度事件 + 魔数嗅探安装）。
/// 防重复：已安装列表里有同一下载地址直接报错。
#[tauri::command]
async fn download_trainer(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    trainer_id: String,
    download_url: String,
) -> Result<gm_core::trainers::install::InstalledTrainer, String> {
    let meta = resolve_install_meta(&state, &trainer_id, &download_url)?;
    let root = trainers_root(&state);
    let sevenz = sevenz_path(&app);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut downloads = state.trainer_downloads.lock().unwrap();
        if downloads.contains_key(&trainer_id) {
            return Err("该修改器正在下载，请先取消或等待完成".to_string());
        }
        downloads.insert(trainer_id.clone(), Arc::clone(&cancel));
    }
    let join = tauri::async_runtime::spawn_blocking(move || {
        download_and_install_blocking(&app, &root, &meta, &sevenz, &cancel)
    })
    .await;
    state.trainer_downloads.lock().unwrap().remove(&trainer_id);
    join.map_err(|e| format!("下载任务失败: {e}"))?
}

/// 就地更新：下载安装最新包，成功后删除旧包（旧包删除失败不影响新版可用）。
#[tauri::command]
async fn update_trainer(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    trainer_id: String,
    download_url: String,
    old_folder: String,
) -> Result<gm_core::trainers::install::InstalledTrainer, String> {
    let meta = resolve_install_meta(&state, &trainer_id, &download_url)?;
    let root = trainers_root(&state);
    let sevenz = sevenz_path(&app);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut downloads = state.trainer_downloads.lock().unwrap();
        if downloads.contains_key(&trainer_id) {
            return Err("该修改器正在下载，请先取消或等待完成".to_string());
        }
        downloads.insert(trainer_id.clone(), Arc::clone(&cancel));
    }
    let join = tauri::async_runtime::spawn_blocking(move || {
        let installed = download_and_install_blocking(&app, &root, &meta, &sevenz, &cancel)?;
        // 新包已就位，旧包尽力清理（失败仅记录，用户可手动删）
        if let Err(e) =
            gm_core::trainers::install::delete_installed(&root, std::path::Path::new(&old_folder))
        {
            eprintln!("[trainers] 旧包清理失败（不影响使用）: {e}");
        }
        Ok(installed)
    })
    .await;
    state.trainer_downloads.lock().unwrap().remove(&trainer_id);
    join.map_err(|e| format!("更新任务失败: {e}"))?
}

/// 取消正在连接/下载的修改器任务；返回是否找到了在途任务。
#[tauri::command]
fn cancel_trainer_download(state: tauri::State<AppState>, trainer_id: String) -> bool {
    let downloads = state.trainer_downloads.lock().unwrap();
    if let Some(flag) = downloads.get(&trainer_id) {
        flag.store(true, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// 启动已安装的修改器（ShellExecute，UAC 由 Windows 按 exe 声明处理）
#[tauri::command]
fn launch_installed_trainer(folder: String) -> Result<(), String> {
    gm_core::trainers::install::launch(std::path::Path::new(&folder))
}

/// 删除已安装的修改器包（引擎侧校验目录必须含有效 manifest 且位于根目录内）
#[tauri::command]
fn delete_installed_trainer(
    state: tauri::State<AppState>,
    folder: String,
) -> Result<(), String> {
    gm_core::trainers::install::delete_installed(&trainers_root(&state), std::path::Path::new(&folder))
}

/// 更新检查：已安装包 vs 目录最新包（判定键 = 下载地址）
#[tauri::command]
fn check_trainer_updates(state: tauri::State<AppState>) -> Vec<gm_core::trainers::TrainerUpdateInfo> {
    state.trainers.read().unwrap().check_updates(&trainers_root(&state))
}

// ======================================================================
// 目录在线更新与 Defender 白名单
// ======================================================================

/// 修改器目录的在线更新地址（GitHub Release 资产；发布流程见 scripts/trainers/README.md）
const CATALOG_UPDATE_URL: &str =
    "https://github.com/LinkJudge/Game-Manage-Center/releases/latest/download/gm-trainers-fling.json";

/// 目录在线更新结果
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogUpdateResult {
    count: usize,
    generated_at: Option<String>,
}

/// 在线更新修改器目录：下载 → 完整校验通过才落盘到应用数据目录 → 整体重载注册表。
/// 校验失败的下载不会影响当前正在使用的目录。
#[tauri::command]
async fn update_trainer_catalog(app: tauri::AppHandle) -> Result<CatalogUpdateResult, String> {
    use tauri::Manager;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let client = download_client()?;
        let json = client
            .get(CATALOG_UPDATE_URL)
            .send()
            .map_err(|e| format!("下载目录失败: {e}"))?
            .error_for_status()
            .map_err(|e| {
                let status = e.status().unwrap_or_default();
                if status == reqwest::StatusCode::NOT_FOUND {
                    "发布页上还没有目录资产（gm-trainers-fling.json），需要先按 scripts/trainers/README.md 的流程发布一次".to_string()
                } else {
                    format!("目录下载被拒绝（{status}）")
                }
            })?
            .text()
            .map_err(|e| format!("读取目录数据失败: {e}"))?;

        // 完整校验（能解析出合法的 fling 来源才落盘）
        let source = gm_core::trainers::fling::FlingSource::from_json(&json)?;
        use gm_core::trainers::TrainerSource as _;
        let generated_at = source.generated_at().map(String::from);
        let count = source.entries().len();

        let dir = state.app_data_dir.join("trainers");
        std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
        let tmp = dir.join("fling.json.tmp");
        std::fs::write(&tmp, &json).map_err(|e| format!("写入失败: {e}"))?;
        std::fs::rename(&tmp, dir.join("fling.json")).map_err(|e| format!("替换失败: {e}"))?;

        // 重载：内置目录在前，应用数据目录在后（后者胜出）
        let (reg, errs) =
            gm_core::trainers::TrainerRegistry::load_dirs(&[trainers_dir(&app), dir]);
        for e in &errs {
            eprintln!("[trainers] {e}");
        }
        *state.trainers.write().unwrap() = reg;
        Ok(CatalogUpdateResult { count, generated_at })
    })
    .await
    .map_err(|e| format!("目录更新任务失败: {e}"))?
}

/// 把修改器根目录加入 Windows Defender 排除项（UAC 提权执行 Add-MpPreference）。
/// 修改器会改游戏内存，常被杀软误删；用户可自行选择是否添加。
#[tauri::command]
fn add_defender_exclusion(state: tauri::State<AppState>) -> Result<(), String> {
    let root = trainers_root(&state);
    // 内层脚本写临时 ps1（路径含空格也安全），外层 powershell 提权运行它并回传退出码
    let ps1 = std::env::temp_dir().join(format!("gm-defender-{}.ps1", std::process::id()));
    std::fs::write(&ps1, format!("Add-MpPreference -ExclusionPath '{}'", root.display()))
        .map_err(|e| format!("写入临时脚本失败: {e}"))?;
    let script = format!(
        "$p = Start-Process powershell -Verb RunAs -Wait -PassThru -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','{}'; exit $p.ExitCode",
        ps1.display()
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map_err(|e| format!("无法运行 PowerShell: {e}"));
    let _ = std::fs::remove_file(&ps1);
    let out = out?;
    if !out.status.success() {
        let code = out.status.code().unwrap_or(-1);
        return Err(if code == 1223 {
            "已取消（未确认 UAC 弹窗）".to_string()
        } else {
            format!("未能添加 Defender 排除项（退出码 {code}），可能受安全策略限制")
        });
    }
    Ok(())
}

#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

// 命令层刻意用扁平的 Option 参数做补丁式更新（只改传入项），参数数随设置增长属预期
#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    backup_path: Option<String>,
    max_backups: Option<u32>,
    compress_backups: Option<bool>,
    auto_backup_enabled: Option<bool>,
    close_to_tray: Option<bool>,
    trainer_path: Option<String>,
    steamgriddb_api_key: Option<String>,
    locale: Option<String>,
    webdav_url: Option<String>,
    webdav_username: Option<String>,
    webdav_password: Option<String>,
    webdav_enabled: Option<bool>,
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
    if let Some(path) = trainer_path {
        settings.trainer_path = if path.trim().is_empty() { None } else { Some(path) };
    }
    if let Some(key) = steamgriddb_api_key {
        settings.steamgriddb_api_key = if key.trim().is_empty() {
            None
        } else {
            Some(key.trim().to_string())
        };
    }
    if let Some(value) = locale {
        // 只接受已知取值；"system" 与空串都归一为跟随系统
        settings.locale = match value.trim() {
            "zh" => Some("zh".to_string()),
            "en" => Some("en".to_string()),
            _ => None,
        };
    }
    if let Some(v) = webdav_url {
        settings.webdav_url = if v.trim().is_empty() {
            None
        } else {
            Some(v.trim().trim_end_matches('/').to_string())
        };
    }
    if let Some(v) = webdav_username {
        settings.webdav_username = if v.trim().is_empty() {
            None
        } else {
            Some(v.trim().to_string())
        };
    }
    if let Some(v) = webdav_password {
        // 密码允许留空（部分服务器匿名可写）；空串视为清除
        settings.webdav_password = if v.is_empty() { None } else { Some(v) };
    }
    if let Some(v) = webdav_enabled {
        settings.webdav_enabled = v;
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

/// 测试 WebDAV 连接：对根目录发 PROPFIND，能拿到 2xx/207 即视为可用
#[tauri::command]
fn test_webdav(
    url: String,
    username: Option<String>,
    password: Option<String>,
) -> Result<webdav::Info, String> {
    let trimmed = url.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err("请先填写 WebDAV 地址".into());
    }
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err("地址需以 http:// 或 https:// 开头".into());
    }
    webdav::test(&webdav::Config {
        url: trimmed,
        username: username.filter(|s| !s.trim().is_empty()),
        password: password.filter(|s| !s.is_empty()),
    })
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
// 环境探测与扫描命令
// ======================================================================

/// 校验一个 SQLite 文件确为有效的存档数据库（返回游戏条数）
fn validate_database(path: &std::path::Path) -> Result<i64, String> {
    let db = gm_core::database::Database::open_readonly(path)?;
    let count = db.total_games()?;
    if count <= 0 {
        return Err("数据库为空".to_string());
    }
    Ok(count)
}

/// 当前存档数据库信息（条数 / 来源 / 更新时间）
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDatabaseInfo {
    pub count: i64,
    /// "user" = 应用内更新过；"bundled" = 随应用分发
    pub source: String,
    /// 文件最后修改时间（RFC3339；内置资源为构建时间）
    #[serde(rename = "updatedAt")]
    pub updated_at: Option<String>,
}

#[tauri::command]
fn save_database_info(state: tauri::State<AppState>) -> Result<SaveDatabaseInfo, String> {
    let path = state.db_path.lock().unwrap().clone();
    let count = validate_database(&path)?;
    let user_db = state.app_data_dir.join("database.db");
    let source = if path == user_db { "user" } else { "bundled" }.to_string();
    let updated_at = std::fs::metadata(&path)
        .ok().and_then(|m| m.modified().ok())
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        ;
    Ok(SaveDatabaseInfo { count, source, updated_at })
}

/// 应用内更新存档数据库：从发布页下载 → 校验 → 原子落盘到应用数据目录
/// → 热切换当前数据库并重建中文名索引。校验失败不影响正在使用的数据库。
#[tauri::command]
async fn update_save_database(app: tauri::AppHandle) -> Result<SaveDatabaseInfo, String> {
    use tauri::Manager;
    const DATABASE_URL: &str =
        "https://github.com/LinkJudge/Game-Manage-Center/releases/latest/download/gm-database.db";
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let client = download_client()?;
        let bytes = client
            .get(DATABASE_URL)
            .send()
            .map_err(|e| format!("下载数据库失败: {e}"))?
            .error_for_status()
            .map_err(|e| {
                let status = e.status().unwrap_or_default();
                if status == reqwest::StatusCode::NOT_FOUND {
                    "发布页上还没有数据库资产（gm-database.db），需要先发布一次".to_string()
                } else {
                    format!("数据库下载被拒绝（{status}）")
                }
            })?
            .bytes()
            .map_err(|e| format!("读取数据库失败: {e}"))?;

        // 校验：先落临时文件再整体验证，失败的下载不碰现有数据库
        let target = state.app_data_dir.join("database.db");
        let tmp = state.app_data_dir.join("database.db.tmp");
        std::fs::write(&tmp, &bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;
        let count = validate_database(&tmp).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("下载数据库校验失败: {e}")
        })?;
        std::fs::rename(&tmp, &target).map_err(|e| format!("替换数据库失败: {e}"))?;

        // 热切换当前数据库 + 重建修改器中文名索引
        *state.db_path.lock().unwrap() = target.clone();
        let zh = gm_core::database::Database::open_readonly(&target)
            .and_then(|db| db.title_zh_pairs())
            .map(gm_core::trainers::ZhNameIndex::build)
            .unwrap_or_else(|e| {
                eprintln!("[database] 中文名索引重建失败: {e}");
                gm_core::trainers::ZhNameIndex::empty()
            });
        *state.trainer_zh.write().unwrap() = zh;

        let updated_at = std::fs::metadata(&target)
            .ok().and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            ;
        Ok(SaveDatabaseInfo {
            count,
            source: "user".to_string(),
            updated_at,
        })
    })
    .await
    .map_err(|e| format!("数据库更新任务失败: {e}"))?
}

#[tauri::command]
async fn scan_installed_games(
    state: tauri::State<'_, AppState>,
    all_accounts: Option<bool>,
) -> Result<gm_core::scanner::ScanOutput, String> {
    let db_path = state.db_path.lock().unwrap().clone();
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
    let db_path = state.db_path.lock().unwrap().clone();
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

// ======================================================================
// 备份与恢复命令
// ======================================================================

#[tauri::command]
async fn backup_game(
    state: tauri::State<'_, AppState>,
    pcgw_id: i64,
    all_accounts: Option<bool>,
) -> Result<String, String> {
    let db_path = state.db_path.lock().unwrap().clone();
    let root = state.backups_root();
    let max = state.max_backups();
    let compress = state.compress();
    tauri::async_runtime::spawn_blocking(move || {
        let db = gm_core::database::Database::open_readonly(&db_path)?;
        let entry = db
            .get_game(pcgw_id)?
            .ok_or_else(|| format!("数据库里没有 pcgw_id = {pcgw_id} 的游戏"))?;
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
    pcgw_id: i64,
) -> Result<Option<gm_core::backup::GameBackups>, String> {
    let root = state.backups_root();
    tauri::async_runtime::spawn_blocking(move || {
        gm_core::backup::list_backups(&root, pcgw_id)
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
}

#[tauri::command]
async fn check_restore_conflict(
    state: tauri::State<'_, AppState>,
    pcgw_id: i64,
    date: String,
) -> Result<gm_core::restore::ConflictInfo, String> {
    let root = state.backups_root();
    let db_path = state.db_path.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = gm_core::database::Database::open_readonly(&db_path)?;
        let entry = db
            .get_game(pcgw_id)?
            .ok_or_else(|| format!("数据库里没有 pcgw_id = {pcgw_id} 的游戏"))?;
        let env = gm_core::detect::detect();
        let home = home_dir();
        let game = gm_core::backup::list_backups(&root, pcgw_id)?
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
    pcgw_id: i64,
    date: String,
    force: Option<bool>,
) -> Result<gm_core::restore::RestoreResult, String> {
    let root = state.backups_root();
    tauri::async_runtime::spawn_blocking(move || {
        let env = gm_core::detect::detect();
        let home = home_dir();
        let game = gm_core::backup::list_backups(&root, pcgw_id)?
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
    pcgw_id: i64,
    date: String,
) -> Result<(), String> {
    let root = state.backups_root();
    tauri::async_runtime::spawn_blocking(move || {
        gm_core::backup::delete_backup(&root, pcgw_id, &date)
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
}

#[tauri::command]
async fn set_backup_permanent(
    state: tauri::State<'_, AppState>,
    pcgw_id: i64,
    date: String,
    permanent: bool,
) -> Result<(), String> {
    let root = state.backups_root();
    tauri::async_runtime::spawn_blocking(move || {
        gm_core::backup::set_backup_permanent(&root, pcgw_id, &date, permanent)
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
    let db_path = state.db_path.lock().unwrap().clone();
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
    let keep: Vec<i64> = output.games.iter().map(|g| g.entry.pcgw_id).collect();
    let mut started = 0usize;
    {
        let mut manager = auto_backup::manager().lock().unwrap();
        manager.retain(&keep); // 停止已不在列表中的游戏（卸载/移除）
        for game in &output.games {
            let id = game.entry.pcgw_id;
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

// 批量操作 / 备份迁移 / 打开目录
// ======================================================================

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub pcgw_id: i64,
    pub title: String,
    pub ok: bool,
    pub message: Option<String>,
}

#[tauri::command]
async fn batch_backup(
    state: tauri::State<'_, AppState>,
    ids: Vec<i64>,
) -> Result<Vec<BatchResult>, String> {
    let db_path = state.db_path.lock().unwrap().clone();
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
                    .ok_or_else(|| format!("数据库里没有 pcgw_id = {id} 的游戏"))?;
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
                pcgw_id: id,
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
    let db_path = state.db_path.lock().unwrap().clone();
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
                pcgw_id: id,
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
    pcgw_id: i64,
) -> Result<(), String> {
    // 只打开已存在的游戏备份目录；绝不创建——未备份的游戏在右键菜单里不出现此项
    let dir = gm_core::backup::game_dir_for(&state.backups_root(), pcgw_id)
        .ok_or_else(|| "该游戏还没有备份记录".to_string())?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// 各游戏的备份数量（右键菜单"是否已备份"的判断依据）
#[tauri::command]
fn backup_counts(state: tauri::State<AppState>) -> std::collections::HashMap<i64, u32> {
    gm_core::backup::backup_counts(&state.backups_root())
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

/// 获取游戏方形封面（带本地磁盘缓存；失败时返回 null，由前端回退字母图标）
#[tauri::command]
async fn get_game_cover(
    state: tauri::State<'_, AppState>,
    title: String,
    pcgw_id: Option<i64>,
    steam_id: Option<String>,
    install_path: Option<String>,
) -> Result<Option<String>, String> {
    let app_data_dir = state.app_data_dir.clone();
    let db_path = state.db_path.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        game_cover::fetch_cover(
            &app_data_dir,
            &db_path,
            &title,
            pcgw_id,
            steam_id,
            install_path,
        )
    })
    .await
    .map_err(|e| format!("封面任务失败: {e}"))?
}

/// 清空已下载的游戏封面缓存
#[tauri::command]
fn clear_game_cover_cache(state: tauri::State<AppState>) -> Result<u64, String> {
    game_cover::clear_cache(&state.app_data_dir)
}

fn steamgriddb_key(state: &AppState) -> Result<String, String> {
    state
        .settings
        .lock()
        .unwrap()
        .steamgriddb_api_key
        .clone()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| "请先在 设置 → 游戏图标 中配置 SteamGridDB API Key".to_string())
}

#[tauri::command]
fn steamgriddb_status(state: tauri::State<AppState>) -> bool {
    steamgriddb_key(&state).is_ok()
}

/// 搜索 SteamGridDB 候选图标
#[tauri::command]
async fn steamgriddb_candidates(
    state: tauri::State<'_, AppState>,
    title: String,
    steam_id: Option<String>,
    pcgw_id: Option<i64>,
) -> Result<steamgriddb::SgdbOptions, String> {
    let api_key = steamgriddb_key(&state)?;
    let db_path = state.db_path.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let appid = game_cover::resolve_steam_appid(
            &db_path,
            &title,
            pcgw_id,
            steam_id.as_deref(),
        );
        steamgriddb::search_candidates(&api_key, &title, appid.as_deref())
    })
    .await
    .map_err(|e| format!("搜索任务失败: {e}"))?
}

/// 应用选中的 SteamGridDB 图标
#[tauri::command]
async fn steamgriddb_apply_cover(
    state: tauri::State<'_, AppState>,
    title: String,
    pcgw_id: Option<i64>,
    steam_id: Option<String>,
    candidate_url: String,
) -> Result<String, String> {
    let app_data_dir = state.app_data_dir.clone();
    let db_path = state.db_path.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        steamgriddb::apply_candidate(
            &app_data_dir,
            &db_path,
            &title,
            pcgw_id,
            steam_id,
            &candidate_url,
        )
    })
    .await
    .map_err(|e| format!("应用图标失败: {e}"))?
}

/// 批量补全缺失封面
#[tauri::command]
async fn steamgriddb_fill_missing(
    state: tauri::State<'_, AppState>,
    requests: Vec<steamgriddb::CoverLookup>,
) -> Result<steamgriddb::FillSummary, String> {
    let api_key = steamgriddb_key(&state)?;
    let app_data_dir = state.app_data_dir.clone();
    let db_path = state.db_path.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        steamgriddb::fill_missing(&api_key, &app_data_dir, &db_path, requests)
    })
    .await
    .map_err(|e| format!("补全任务失败: {e}"))?
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

            // 产品改名一次性迁移：旧标识符数据目录 → 新标识符（设置/自定义游戏/修改器/备份跟随）
            if let Some(old_dir) = app_data_dir.parent().map(|p| p.join("com.gamemanager.app")) {
                if old_dir.exists() && !app_data_dir.exists() {
                    if let Err(e) = std::fs::rename(&old_dir, &app_data_dir) {
                        // 跨盘等场景退化为复制（保留旧目录兜底）
                        gm_core::backup::copy_dir_recursive(&old_dir, &app_data_dir)
                            .map_err(|ce| format!("迁移失败: rename {e}; copy {ce}"))?;
                    }
                    eprintln!("[migrate] 数据目录已迁移: {} → {}", old_dir.display(), app_data_dir.display());
                }
            }

            let settings_path = app_data_dir.join("settings.json");
            let settings = AppSettings::load(&settings_path);
            let mut db_path = database_path(app.handle()).unwrap_or_else(|_| PathBuf::new());

            // 用户更新过的存档数据库优先于内置资源（更新功能写入 app_data/database.db）
            let user_db = app_data_dir.join("database.db");
            if user_db != db_path && user_db.exists() {
                let valid = gm_core::database::Database::open_readonly(&user_db)
                    .and_then(|db| db.total_games())
                    .map(|n| n > 0)
                    .unwrap_or(false);
                if valid {
                    eprintln!("[database] 使用用户更新的存档数据库: {}", user_db.display());
                    db_path = user_db;
                } else {
                    eprintln!("[database] 用户数据库损坏，回退内置资源");
                }
            }

            // 修改器目录：内置 resources/trainers + 用户更新目录 app_data/trainers
            // （后者覆盖同来源，缺目录降级为空注册表）
            let (trainers, trainer_errors) = gm_core::trainers::TrainerRegistry::load_dirs(&[
                trainers_dir(app.handle()),
                app_data_dir.join("trainers"),
            ]);
            for e in &trainer_errors {
                eprintln!("[trainers] {e}");
            }
            let trainer_zh = gm_core::database::Database::open_readonly(&db_path)
                .and_then(|db| db.title_zh_pairs())
                .map(gm_core::trainers::ZhNameIndex::build)
                .unwrap_or_else(|e| {
                    eprintln!("[trainers] 中文名索引构建失败: {e}");
                    gm_core::trainers::ZhNameIndex::empty()
                });

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
                db_path: Mutex::new(db_path),
                settings_path,
                custom_games_path: app_data_dir.join("custom_games.json"),
                app_data_dir: app_data_dir.clone(),
                trainers: std::sync::RwLock::new(trainers),
                trainer_zh: std::sync::RwLock::new(trainer_zh),
                trainer_downloads: Mutex::new(HashMap::new()),
            });

            // 旧版备份布局迁移：数字 id 目录 → 游戏名目录（game.json 声明归属）
            {
                let root = app.state::<AppState>().backups_root();
                let moved = gm_core::backup::migrate_numeric_game_dirs(&root);
                if moved > 0 {
                    eprintln!("[backup-migrate] 已迁移 {moved} 个游戏的备份目录到游戏名布局");
                }
            }

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
                .tooltip("Game Manage Center")
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
            check_app_update,
            list_modules,
            list_trainer_sources,
            search_trainers,
            match_trainers,
            list_installed_trainers,
            download_trainer,
            update_trainer,
            cancel_trainer_download,
            launch_installed_trainer,
            delete_installed_trainer,
            check_trainer_updates,
            update_trainer_catalog,
            add_defender_exclusion,
            test_webdav,
            scan_installed_games,
            save_database_info,
            update_save_database,
            scan_deep_games,
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
            backup_counts,
            open_save_folder,
            get_game_cover,
            clear_game_cover_cache,
            steamgriddb_status,
            steamgriddb_candidates,
            steamgriddb_apply_cover,
            steamgriddb_fill_missing,
            get_settings,
            save_settings,
            pick_folder,
            get_autostart,
            set_autostart
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
