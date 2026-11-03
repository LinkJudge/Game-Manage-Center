//! 修改器安装引擎 —— 把下载好的包落到修改器根目录并写自研 manifest。
//!
//! 来源站点的传输形态不统一（实测同为风灵月影，有的包是 RAR，有的是裸 exe），
//! 因此按魔数嗅探分发：`MZ` 视为单文件修改器直接安装；`Rar!`/`PK`/7z 签名
//! 视为压缩包，调用 7-Zip 命令行解压（`7z.exe`/`7z.dll` 随应用资源分发，
//! 路径由调用方传入——与 backup.rs 调 reg.exe 同一模式）。
//!
//! 安装布局（gm 自研，与存档备份布局风格一致）：
//! ```text
//! <修改器根>/<游戏名>/<包目录名>/
//!   manifest.json   ← 自述清单（gm.trainer-manifest/1）
//!   <解压出的修改器文件>
//! ```

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

/// manifest 格式版本（加载时不匹配即忽略该目录）
pub const MANIFEST_SCHEMA: &str = "gm.trainer-manifest/1";
const MANIFEST_FILE: &str = "manifest.json";

/// 下载包安装元数据（来自 catalog 条目 + 选中的附件）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerInstallMeta {
    pub trainer_id: String,
    pub source_id: String,
    pub game_name: String,
    #[serde(default)]
    pub zh_name: Option<String>,
    /// catalog 附件名（无扩展名，如 Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG）
    pub file: String,
    pub download_url: String,
    pub page_url: String,
    #[serde(default)]
    pub date_added: Option<String>,
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub options_count: Option<u32>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

/// 安装后的自述清单（落在包目录 manifest.json）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerManifest {
    pub schema: String,
    #[serde(flatten)]
    pub meta: TrainerInstallMeta,
    /// 安装时间（RFC3339）
    pub installed_at: String,
    /// 主修改器 exe（包内相对路径；多文件包无法判定时为 None，仅可打开目录）
    pub primary_exe: Option<String>,
    /// 包内全部文件（相对路径，安装校验与展示用）
    pub files: Vec<String>,
}

/// 扫描到的已安装修改器（list_installed_trainers 返回）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledTrainer {
    pub manifest: TrainerManifest,
    /// 包目录绝对路径
    pub folder: String,
    /// 游戏库官方简体名（按 manifest 的 game_name 归一化匹配；查不到为 None）
    #[serde(default)]
    pub zh_name_resolved: Option<String>,
    /// 游戏库官方繁体名
    #[serde(default)]
    pub zh_hant_resolved: Option<String>,
}

/// 安装结果
#[derive(Debug)]
pub struct InstallOutcome {
    pub folder: PathBuf,
    pub manifest: TrainerManifest,
}

// ======================================================================
// 命名与嗅探
// ======================================================================

/// 游戏目录名：复用 sanitize_folder（去 Windows 非法字符）并压缩空白
pub fn game_folder_name(game_name: &str) -> String {
    let cleaned = crate::sanitize_folder(game_name);
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 包目录名 = catalog 附件名（剥掉真实压缩包扩展名，其余原样保留以保证
/// 可溯源与唯一性）+ Windows 非法字符清洗。
/// 注意不能走 `file_stem`：它会把 "Plus.44.Trainer-FLiNG" 的最后一段当扩展名
/// 剥掉，甚至啃掉纯数字结尾的版本号。
pub fn package_folder_name(file: &str) -> String {
    let name = Path::new(file)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| file.to_string());
    let lower = name.to_lowercase();
    let stem = [".zip", ".rar", ".7z", ".exe"]
        .iter()
        .find(|ext| lower.ends_with(**ext))
        .map(|ext| &name[..name.len() - ext.len()])
        .unwrap_or(&name);
    crate::sanitize_folder(stem).trim().to_string()
}

/// 包传输形态（按魔数嗅探，不信任扩展名与 content-type）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageKind {
    /// 裸 PE（MZ 头）——本身就是修改器
    Exe,
    /// RAR! 签名
    Rar,
    /// PK\x03\x04（zip）
    Zip,
    /// 7z 签名
    SevenZ,
    Unknown,
}

pub fn sniff_package_kind(bytes: &[u8]) -> PackageKind {
    if bytes.starts_with(b"Rar!") {
        PackageKind::Rar
    } else if bytes.starts_with(b"PK\x03\x04") {
        PackageKind::Zip
    } else if bytes.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        PackageKind::SevenZ
    } else if bytes.starts_with(b"MZ") {
        PackageKind::Exe
    } else {
        PackageKind::Unknown
    }
}

// ======================================================================
// 安装
// ======================================================================

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn read_header(path: &Path) -> Result<Vec<u8>, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("打开下载文件失败: {e}"))?;
    let mut buf = vec![0u8; 8];
    let n = f.read(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(n);
    Ok(buf)
}

/// 找主修改器 exe：优先名字含 "trainer" 的（取最大者），否则任一 exe；没有则 None。
/// 返回相对 staging 根的路径。
fn find_primary_exe(dir: &Path) -> Option<String> {
    let mut candidates: Vec<(PathBuf, u64, bool)> = Vec::new();
    collect_exe_candidates(dir, dir, &mut candidates);
    if candidates.is_empty() {
        return None;
    }
    let trainer_hits: Vec<_> = candidates.iter().filter(|(_, _, is_trainer)| *is_trainer).collect();
    let pool = if trainer_hits.is_empty() { candidates.iter().collect() } else { trainer_hits };
    let best = pool.iter().max_by_key(|(_, size, _)| *size)?;
    Some(best.0.to_string_lossy().replace('\\', "/"))
}

fn collect_exe_candidates(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, u64, bool)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_exe_candidates(root, &path, out);
            continue;
        }
        let is_exe = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("exe"))
            .unwrap_or(false);
        if !is_exe {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let is_trainer = stem.contains("trainer");
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        out.push((rel, size, is_trainer));
    }
}

/// 解压任意 7-Zip 支持的格式到 dest（调用方保证 dest 已创建）
fn extract_with_sevenz(sevenz_exe: &Path, archive: &Path, dest: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut cmd = Command::new(sevenz_exe);
    cmd.arg("x")
        .arg("-y")
        .arg(format!("-o{}", dest.display()))
        .arg(archive)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("无法运行 7z（{}）: {e}", sevenz_exe.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("解压失败（7z 退出码 {}）: {}", out.status, err.trim()));
    }
    Ok(())
}

/// 安装一个下载完成的包：
/// 1. 目标目录 = root/<游戏名>/<包目录名>；已存在 → 报错（防重复由调用方先查 url）
/// 2. 魔数嗅探：exe 直接落位；压缩包用 7z 解到 staging
/// 3. 判定主 exe → 目录改名落位 → 写 manifest.json
///
/// 全程先 staging 后落位，任何失败清理 staging，不留半成品。
pub fn install_package(
    downloaded: &Path,
    meta: &TrainerInstallMeta,
    root: &Path,
    sevenz_exe: &Path,
) -> Result<InstallOutcome, String> {
    let game_dir = root.join(game_folder_name(&meta.game_name));
    let package_dir = game_dir.join(package_folder_name(&meta.file));
    if package_dir.exists() {
        return Err(format!(
            "同名包目录已存在：{}（如需重装请先删除）",
            package_dir.display()
        ));
    }
    let staging = root.join(format!(".staging-{}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()));
    std::fs::create_dir_all(&staging).map_err(|e| format!("创建安装临时目录失败: {e}"))?;

    let result = (|| -> Result<InstallOutcome, String> {
        let kind = sniff_package_kind(&read_header(downloaded)?);
        match kind {
            PackageKind::Exe => {
                let name = Path::new(&meta.file)
                    .file_stem()
                    .map(|s| format!("{}.exe", s.to_string_lossy()))
                    .unwrap_or_else(|| "trainer.exe".to_string());
                std::fs::copy(downloaded, staging.join(&name))
                    .map_err(|e| format!("落位修改器失败: {e}"))?;
            }
            PackageKind::Rar | PackageKind::Zip | PackageKind::SevenZ => {
                extract_with_sevenz(sevenz_exe, downloaded, &staging)?;
            }
            PackageKind::Unknown => {
                return Err("下载内容无法识别（既不是修改器也不是压缩包），可能被杀软截留".to_string());
            }
        }

        let primary = find_primary_exe(&staging);
        let mut files = Vec::new();
        collect_all_files(&staging, &staging, &mut files);

        std::fs::create_dir_all(&game_dir).map_err(|e| e.to_string())?;

        let manifest = TrainerManifest {
            schema: MANIFEST_SCHEMA.to_string(),
            meta: meta.clone(),
            installed_at: now_rfc3339(),
            primary_exe: primary.clone(),
            files,
        };
        let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
        std::fs::write(staging.join(MANIFEST_FILE), json).map_err(|e| e.to_string())?;
        std::fs::rename(&staging, &package_dir).map_err(|e| format!("目录落位失败: {e}"))?;
        Ok(InstallOutcome {
            folder: package_dir,
            manifest,
        })
    })();

    match result {
        Ok(outcome) => Ok(outcome),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

fn collect_all_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_all_files(root, &path, out);
        } else {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

// ======================================================================
// 已安装扫描
// ======================================================================

/// 扫描修改器根目录下全部已安装包（root/<游戏>/<包>/manifest.json 两层结构）
pub fn scan_installed(root: &Path) -> Vec<InstalledTrainer> {
    let mut out = Vec::new();
    let Ok(games) = std::fs::read_dir(root) else { return out };
    for game in games.flatten() {
        if !game.path().is_dir() {
            continue;
        }
        let Ok(packages) = std::fs::read_dir(game.path()) else { continue };
        for package in packages.flatten() {
            let dir = package.path();
            if !dir.is_dir() {
                continue;
            }
            let manifest_path = dir.join(MANIFEST_FILE);
            let Ok(json) = std::fs::read_to_string(&manifest_path) else { continue };
            let Ok(manifest) = serde_json::from_str::<TrainerManifest>(&json) else { continue };
            if manifest.schema != MANIFEST_SCHEMA {
                continue;
            }
            out.push(InstalledTrainer {
                manifest,
                folder: dir.to_string_lossy().to_string(),
                // 两语言名字由 list 命令按游戏库回填，扫描阶段先置空
                zh_name_resolved: None,
                zh_hant_resolved: None,
            });
        }
    }
    out.sort_by(|a, b| {
        a.manifest
            .meta
            .game_name
            .cmp(&b.manifest.meta.game_name)
            .then_with(|| b.manifest.installed_at.cmp(&a.manifest.installed_at))
    });
    out
}

/// 已安装包的下载地址集合（防重复下载的快速判断）
pub fn installed_url_index(root: &Path) -> std::collections::HashSet<String> {
    scan_installed(root)
        .into_iter()
        .map(|t| t.manifest.meta.download_url)
        .collect()
}

// ======================================================================
// 已安装管理：启动 / 删除
// ======================================================================

/// 读取包目录的 manifest（schema 校验），供启动/删除前置校验
fn read_manifest(folder: &Path) -> Result<TrainerManifest, String> {
    let path = folder.join(MANIFEST_FILE);
    let json = std::fs::read_to_string(&path)
        .map_err(|_| "该目录不是已安装的修改器包（缺少 manifest.json）".to_string())?;
    let manifest: TrainerManifest =
        serde_json::from_str(&json).map_err(|e| format!("manifest 损坏: {e}"))?;
    if manifest.schema != MANIFEST_SCHEMA {
        return Err(format!("manifest 格式不符：{}", manifest.schema));
    }
    Ok(manifest)
}

/// 启动已安装的修改器（走 ShellExecute：双击同款行为——exe 自带
/// requireAdministrator 时由 Windows 弹 UAC，普通 exe 直接运行）。
pub fn launch(folder: &Path) -> Result<(), String> {
    let manifest = read_manifest(folder)?;
    let Some(rel) = &manifest.primary_exe else {
        return Err("该包没有可执行主程序，请打开目录手动选择".to_string());
    };
    let exe_path = folder.join(rel);
    if !exe_path.exists() {
        return Err(format!("主程序不存在：{rel}（可能被杀软删除）"));
    }
    launch_exe(&exe_path)
}

#[cfg(windows)]
fn launch_exe(exe_path: &Path) -> Result<(), String> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteExW;
    use windows_sys::Win32::UI::Shell::SHELLEXECUTEINFOW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file_w: Vec<u16> = exe_path
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect();
    let verb_w: Vec<u16> = "open".encode_utf16().chain(once(0)).collect();

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.lpVerb = verb_w.as_ptr();
    info.lpFile = file_w.as_ptr();
    info.nShow = SW_SHOWNORMAL;

    // 成功返回非 0；失败时通过 GetLastError 取错误码
    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        let err = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        return Err(format!(
            "启动失败（Win32 错误码 {err}）。若为需要管理员权限的修改器，请在 UAC 弹窗中确认"
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn launch_exe(_exe_path: &Path) -> Result<(), String> {
    Err("修改器启动仅支持 Windows".to_string())
}

/// 删除一个已安装的修改器包。安全约束：目录必须位于修改器根目录之下、
/// 含有效 manifest（防止误删任意目录）。
pub fn delete_installed(root: &Path, folder: &Path) -> Result<(), String> {
    let root_canon = root
        .canonicalize()
        .map_err(|_| "修改器根目录不存在".to_string())?;
    let folder_canon = folder
        .canonicalize()
        .map_err(|_| format!("目录不存在：{}", folder.display()))?;
    if folder_canon == root_canon || !folder_canon.starts_with(&root_canon) {
        return Err("只能删除修改器根目录内的包".to_string());
    }
    read_manifest(&folder_canon)?;
    std::fs::remove_dir_all(&folder_canon).map_err(|e| {
        format!("删除失败：{e}（修改器可能正在运行，请先退出）")
    })?;
    // 最后一个包删除后清理空的游戏目录（与备份侧 remove_game_dir_if_empty 同款策略）
    if let Some(parent) = folder_canon.parent() {
        if parent != root_canon
            && parent.starts_with(&root_canon)
            && std::fs::read_dir(parent).map(|mut d| d.next().is_none()).unwrap_or(false)
        {
            let _ = std::fs::remove_dir(parent);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(file: &str) -> TrainerInstallMeta {
        TrainerInstallMeta {
            trainer_id: format!("fling/{file}"),
            source_id: "fling".to_string(),
            game_name: "Test Game: Origins".to_string(),
            zh_name: None,
            file: file.to_string(),
            download_url: format!("https://flingtrainer.com/downloads/{file}"),
            page_url: "https://flingtrainer.com/trainer/x/".to_string(),
            date_added: Some("2025-10-17".to_string()),
            game_version: Some("1.0".to_string()),
            options_count: Some(13),
            size_bytes: Some(944128),
        }
    }

    /// 构造一个含给定成员的 zip（zip crate deflate）
    fn make_zip(path: &Path, members: &[(&str, &[u8])]) {
        use std::io::Write;
        let f = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("readme.txt", opts).unwrap();
        zip.write_all(b"n/a").unwrap();
        for (name, content) in members {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }

    /// 最小 PE：MZ 头 + 尾部填充
    fn make_exe(path: &Path, size: usize) {
        let mut buf = vec![0u8; size];
        buf[0] = b'M';
        buf[1] = b'Z';
        std::fs::write(path, buf).unwrap();
    }

    fn sevenz_fixture() -> PathBuf {
        // 优先用应用自带的 7z.exe；CI/其他机器没有则跳过
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/bin/7z.exe");
        if p.exists() { p } else { panic!("测试需要 resources/bin/7z.exe") }
    }

    #[test]
    fn naming_rules() {
        assert_eq!(game_folder_name("Black Myth: Wukong"), "Black Myth Wukong");
        assert_eq!(game_folder_name("  DOOM    Eternal  "), "DOOM Eternal");
        assert_eq!(package_folder_name("Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG"), "Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG");
        assert_eq!(package_folder_name("Game.Name.v1.0.Trainer-FLiNG.zip"), "Game.Name.v1.0.Trainer-FLiNG");
        assert_eq!(package_folder_name("bad:name?.v1"), "badname.v1");
    }

    #[test]
    fn sniff_by_magic_not_extension() {
        assert_eq!(sniff_package_kind(b"MZ\x90\x00"), PackageKind::Exe);
        assert_eq!(sniff_package_kind(b"Rar!\x1a\x07"), PackageKind::Rar);
        assert_eq!(sniff_package_kind(b"PK\x03\x04...."), PackageKind::Zip);
        assert_eq!(sniff_package_kind(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]), PackageKind::SevenZ);
        assert_eq!(sniff_package_kind(b"<html>"), PackageKind::Unknown);
    }

    #[test]
    fn install_zip_package_with_primary_exe_detection() {
        let root = std::env::temp_dir().join(format!("gm-inst-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dl = root.join("dl.bin");
        make_zip(&dl, &[
            ("Final Fantasy VIII Plus 13 Trainer.exe", &[0u8; 5000][..]),
            ("Readme.txt", b"notes".as_slice()),
        ]);

        let outcome = install_package(&dl, &meta("Test.Game.v1.0.Plus.13.Trainer-FLiNG"), &root, &sevenz_fixture()).unwrap();
        assert_eq!(outcome.folder.file_name().unwrap(), "Test.Game.v1.0.Plus.13.Trainer-FLiNG");
        assert!(outcome.folder.join("Readme.txt").exists());
        assert_eq!(outcome.manifest.primary_exe.as_deref(), Some("Final Fantasy VIII Plus 13 Trainer.exe"));
        assert_eq!(outcome.manifest.schema, MANIFEST_SCHEMA);
        assert_eq!(outcome.manifest.meta.options_count, Some(13));
        assert_eq!(outcome.manifest.files.len(), 2);
        // manifest 可被扫描读回
        let installed = scan_installed(&root);
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].manifest.meta.download_url, "https://flingtrainer.com/downloads/Test.Game.v1.0.Plus.13.Trainer-FLiNG");
        assert_eq!(installed_url_index(&root).len(), 1);
        // 重复安装同包名 → 报错
        assert!(install_package(&dl, &meta("Test.Game.v1.0.Plus.13.Trainer-FLiNG"), &root, &sevenz_fixture()).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn manifest_failure_does_not_leave_a_published_package() {
        let root = std::env::temp_dir().join(format!("gm-inst-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let dl = root.join("fixture.zip");
        make_zip(&dl, &[("manifest.json/blocked.txt", b"fixture"), ("trainer.exe", b"MZfixture")]);
        let result = install_package(&dl, &meta("manifest-failure"), &root, &sevenz_fixture());
        assert!(result.is_err());
        assert!(!root.join("Test Game Origins/manifest-failure").exists());
        assert!(!std::fs::read_dir(&root).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with(".staging-")));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn install_bare_exe_prefers_trainer_named_largest() {
        let root = std::env::temp_dir().join(format!("gm-inst-exe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dl = root.join("dl2.bin");
        make_exe(&dl, 3000);

        let outcome = install_package(&dl, &meta("Elden.Ring.Nightreign.v1.01-v1.03.1.Plus.26.Trainer-FLiNG"), &root, &sevenz_fixture()).unwrap();
        let exe = outcome.manifest.primary_exe.unwrap();
        assert!(exe.ends_with(".exe"));
        assert!(exe.contains("Elden.Ring.Nightreign"));
        // 目录里只有那一个文件
        assert_eq!(outcome.manifest.files.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn failed_install_leaves_no_staging_or_package() {
        let root = std::env::temp_dir().join(format!("gm-inst-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dl = root.join("bad.bin");
        std::fs::write(&dl, b"<html>blocked by antivirus</html>").unwrap();
        let err = install_package(&dl, &meta("X.Game.v1.Trainer-FLiNG"), &root, &sevenz_fixture()).unwrap_err();
        assert!(err.contains("无法识别"), "{err}");
        // 无 staging 残留、无包目录
        assert!(scan_installed(&root).is_empty());
        let entries: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().collect();
        assert!(entries.iter().all(|e| e.file_name() == "bad.bin"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
