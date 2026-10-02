//! 自动备份管理器 —— 全局自动备份的执行层
//!
//! 开启全局开关后，为每个已安装游戏挂两条线程：
//! - watcher：notify 递归监控存档路径，变化立即备份，随后 10 秒冷却；
//!   冷却期间发生的改动在冷却结束后补一次备份（pending 语义）
//! - game_exit：UU 加速器式——监测游戏进程，退出瞬间自动备份一次
//!   （此时存档刚好从内存落盘，是最新最完整的时机）
//!
//! 游戏列表由 lib.rs 的 refresh_auto_backup_blocking 依据扫描结果维护
//! （retain 停止已卸载/移除的游戏），应用启动时自动恢复。
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const WATCHER_COOLDOWN_MS: u64 = 10_000;

#[derive(Default)]
pub struct AutoBackupManager {
    /// 每个游戏可同时挂 watcher + game_exit 两条线程，各持一个停止标志
    stops: HashMap<i64, Vec<Arc<AtomicBool>>>,
}

impl AutoBackupManager {
    fn add_flag(&mut self, wiki_page_id: i64) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.stops
            .entry(wiki_page_id)
            .or_default()
            .push(flag.clone());
        flag
    }

    /// 为单个游戏启动"存档变化即备份"线程
    pub fn start_watcher(
        &mut self,
        wiki_page_id: i64,
        watch_paths: Vec<PathBuf>,
        db_path: PathBuf,
        backups_root: PathBuf,
        max_backups: u32,
        compress: bool,
    ) {
        if watch_paths.is_empty() {
            return;
        }
        let stop = self.add_flag(wiki_page_id);
        std::thread::spawn(move || {
            run_watcher(wiki_page_id, watch_paths, db_path, backups_root, max_backups, compress, stop);
        });
    }

    /// 为单个游戏启动"游戏退出即备份"线程
    pub fn start_exit(
        &mut self,
        wiki_page_id: i64,
        exe_names: Vec<String>,
        db_path: PathBuf,
        backups_root: PathBuf,
        max_backups: u32,
        compress: bool,
    ) {
        if exe_names.is_empty() {
            return;
        }
        let stop = self.add_flag(wiki_page_id);
        std::thread::spawn(move || {
            run_game_exit(wiki_page_id, exe_names, db_path, backups_root, max_backups, compress, stop);
        });
    }

    pub fn stop(&mut self, wiki_page_id: i64) {
        if let Some(flags) = self.stops.remove(&wiki_page_id) {
            for flag in flags {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }

    pub fn stop_all(&mut self) {
        for (_, flags) in self.stops.drain() {
            for flag in flags {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }

    /// 只保留 keep 中的游戏，其余停止
    pub fn retain(&mut self, keep: &[i64]) {
        let stale: Vec<i64> = self
            .stops
            .keys()
            .filter(|id| !keep.contains(id))
            .copied()
            .collect();
        for id in stale {
            self.stop(id);
        }
    }

    pub fn is_active(&self, wiki_page_id: i64) -> bool {
        self.stops.contains_key(&wiki_page_id)
    }
}

static MANAGER: OnceLock<Mutex<AutoBackupManager>> = OnceLock::new();

pub fn manager() -> &'static Mutex<AutoBackupManager> {
    MANAGER.get_or_init(|| Mutex::new(AutoBackupManager::default()))
}

/// 单次自动备份：现场解析存档路径 → 备份（与手动备份共用同一引擎）
pub fn run_once(
    wiki_page_id: i64,
    db_path: &Path,
    backups_root: &Path,
    max_backups: u32,
    compress: bool,
) -> Result<(), String> {
    let db = gm_core::database::Database::open_readonly(db_path)?;
    let entry = db
        .get_game(wiki_page_id)?
        .ok_or_else(|| format!("数据库里没有 wiki_page_id = {wiki_page_id} 的游戏"))?;
    let env = gm_core::detect::detect();
    let home = home_dir();
    let scanned = gm_core::scanner::scan_game(&entry, &env, &home, false);
    if scanned.resolved_files.is_empty() {
        return Err("没有找到可备份的存档文件".to_string());
    }
    gm_core::backup::backup_game(&entry, &scanned.resolved_files, backups_root, max_backups, compress)?;
    Ok(())
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// 收集游戏安装目录内的 exe 文件名（深度 ≤2，用于进程监测）
pub fn collect_exe_names(install_path: &Path) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(dir: &Path, depth: u32, out: &mut Vec<String>) {
        if depth > 2 || out.len() >= 100 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, depth + 1, out);
            } else if p
                .extension()
                .map(|x| x.eq_ignore_ascii_case("exe"))
                .unwrap_or(false)
            {
                if let Some(n) = p.file_name() {
                    let n = n.to_string_lossy().to_string();
                    if !out.contains(&n) {
                        out.push(n);
                    }
                }
            }
        }
    }
    walk(install_path, 0, &mut out);
    out
}

/// UU 加速器式：轮询进程列表，游戏从"运行中"变为"已退出"的瞬间备份一次。
/// 之后若游戏再次启动并退出，会再次备份（每次游玩一份）。
fn run_game_exit(
    wiki_page_id: i64,
    exe_names: Vec<String>,
    db_path: PathBuf,
    backups_root: PathBuf,
    max_backups: u32,
    compress: bool,
    stop: Arc<AtomicBool>,
) {
    use sysinfo::System;

    if exe_names.is_empty() {
        eprintln!("[auto-backup {wiki_page_id}] 安装目录里没有 exe，game_exit 退出");
        return;
    }
    eprintln!(
        "[auto-backup {wiki_page_id}] game_exit 已启动，监测 {} 个进程名",
        exe_names.len()
    );

    let mut sys = System::new();
    let mut was_running = false;

    while !stop.load(Ordering::SeqCst) {
        sys.refresh_processes();
        let running = sys.processes().values().any(|p| {
            exe_names
                .iter()
                .any(|exe| p.name().eq_ignore_ascii_case(exe))
        });

        if was_running && !running {
            // 游戏退出瞬间 → 备份一次
            match run_once(wiki_page_id, &db_path, &backups_root, max_backups, compress) {
                Ok(()) => eprintln!("[auto-backup {wiki_page_id}] 游戏退出，已自动备份"),
                Err(e) => eprintln!("[auto-backup {wiki_page_id}] 退出备份失败: {e}"),
            }
        }
        was_running = running;

        // 分片睡眠：既省 CPU 又能及时响应停止
        for _ in 0..20 {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
    }
    eprintln!("[auto-backup {wiki_page_id}] game_exit 已停止");
}

fn run_watcher(
    wiki_page_id: i64,
    paths: Vec<PathBuf>,
    db_path: PathBuf,
    backups_root: PathBuf,
    max_backups: u32,
    compress: bool,
    stop: Arc<AtomicBool>,
) {
    use notify::Watcher;

    if paths.is_empty() {
        eprintln!("[auto-backup {wiki_page_id}] 没有可监控的存档路径，watcher 退出");
        return;
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[auto-backup {wiki_page_id}] 创建 watcher 失败: {e}");
            return;
        }
    };
    for path in &paths {
        let target = if path.is_dir() { path.clone() } else { path.parent().map(PathBuf::from).unwrap_or_else(|| path.clone()) };
        if let Err(e) = watcher.watch(&target, notify::RecursiveMode::Recursive) {
            eprintln!("[auto-backup {wiki_page_id}] 监控 {target:?} 失败: {e}");
        }
    }
    eprintln!("[auto-backup {wiki_page_id}] watcher 已启动，监控 {} 个路径", paths.len());

    // 冷却状态机：变化立即备份 → 10 秒冷却 → 冷却期间有改动则补一次
    let cooldown = Duration::from_millis(WATCHER_COOLDOWN_MS);
    let mut cooldown_until = Instant::now();
    let mut pending = false;

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let now = Instant::now();
        let wait = if now < cooldown_until {
            cooldown_until - now
        } else {
            Duration::from_secs(2)
        };
        match rx.recv_timeout(wait) {
            Ok(_) => {
                let now = Instant::now();
                if now < cooldown_until {
                    pending = true;
                } else {
                    if let Err(e) = run_once(wiki_page_id, &db_path, &backups_root, max_backups, compress) {
                        eprintln!("[auto-backup {wiki_page_id}] 监控备份失败: {e}");
                    }
                    cooldown_until = Instant::now() + cooldown;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if pending && Instant::now() >= cooldown_until {
                    pending = false;
                    if let Err(e) = run_once(wiki_page_id, &db_path, &backups_root, max_backups, compress) {
                        eprintln!("[auto-backup {wiki_page_id}] 冷却后补备份失败: {e}");
                    }
                    cooldown_until = Instant::now() + cooldown;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    eprintln!("[auto-backup {wiki_page_id}] watcher 已停止");
}
