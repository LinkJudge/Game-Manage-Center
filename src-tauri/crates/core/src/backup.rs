//! 备份引擎：实例目录布局、清单读写、压缩打包、轮换与固定。
//!
//! 目录布局：`<backups_root>/<pcgw_id>/<YYYY-MM-DD_HH-mm-ss>/…`，
//! 实例名即创建时间戳（精确到秒，同秒重名自动追加序号）。
//! 每个实例内的 manifest.json 是唯一索引；早期版本实例使用 backup_info.json，
//! 仍只读兼容——首次修改（固定/补写大小）时自动升级为 manifest.json。

use crate::resolve::ResolvedPath;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 当前格式的实例清单文件名
pub const MANIFEST_FILE: &str = "manifest.json";
/// 早期格式的实例清单文件名（只读兼容，写入时迁移）
pub const LEGACY_MANIFEST_FILE: &str = "backup_info.json";
/// 游戏备份目录的归属索引（game.json）——目录名用游戏名后，靠它识别归属与防碰撞
pub const GAME_INDEX_FILE: &str = "game.json";

/// game.json：游戏备份目录的归属声明
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameIndex {
    pub schema: String,
    /// PCGamingWiki 页面 ID。新版本写出 `pcgwId`；
    /// 旧版本写的是 `wikiPageId`，用 alias 保证既有备份目录仍能被识别。
    #[serde(alias = "wikiPageId", alias = "wiki_page_id")]
    pub pcgw_id: i64,
    pub title: String,
}

const GAME_INDEX_SCHEMA: &str = "gm.game-index/1";

/// 备份根目录下的游戏目录名：标题去 Windows 非法字符并压缩空白
pub fn game_dir_name(title: &str) -> String {
    crate::sanitize_folder(title)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 读取目录的归属索引（不存在或损坏 → None）
fn read_game_index(game_dir: &Path) -> Option<GameIndex> {
    let text = fs::read_to_string(game_dir.join(GAME_INDEX_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_game_index(game_dir: &Path, index: &GameIndex) -> Result<(), String> {
    write_json_atomic(&game_dir.join(GAME_INDEX_FILE), index)
}

/// 找出**所有**声明归属于该游戏的备份目录。
///
/// 正常情况只有一个。但游戏英文名改过之后，备份目录名会跟着变
/// （目录名 = `game_dir_name(title)`），旧目录仍然留在磁盘上，
/// 于是一个 `pcgw_id` 可能对应多个目录。只认第一个会让用户
/// 「改名前的备份凭空消失」，所以读取侧必须把所有目录都算进来。
///
/// 写操作（新建备份）仍只用一个目录，见 `game_dir_for`。
pub fn game_dirs_for(backups_root: &Path, pcgw_id: i64) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let numeric = backups_root.join(pcgw_id.to_string());
    if numeric.is_dir() {
        found.push(numeric); // 旧布局（未迁移时兜底）
    }
    if let Ok(entries) = fs::read_dir(backups_root) {
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() || found.contains(&dir) {
                continue;
            }
            if read_game_index(&dir).map(|i| i.pcgw_id) == Some(pcgw_id) {
                found.push(dir);
            }
        }
    }
    found
}

/// 解析 pcgw_id 对应的游戏目录：扫描根目录下各 game.json 归属匹配。
/// 兼容旧布局（目录名为数字 id）：数字目录直接匹配。
///
/// 只有一个目录时就是它；有多个（改名遗留）时取第一个，用于写入新备份。
pub fn game_dir_for(backups_root: &Path, pcgw_id: i64) -> Option<PathBuf> {
    game_dirs_for(backups_root, pcgw_id).into_iter().next()
}

/// 旧布局迁移：数字命名的游戏目录 → 游戏名目录。
/// 标题取实例清单（目录式读 manifest，纯 zip 实例读包内清单）；
/// 空目录（旧版"打开备份文件夹"误建）与只剩 game.json 的空壳（实例删尽）
/// 直接删除。返回迁移的目录数。
pub fn migrate_numeric_game_dirs(backups_root: &Path) -> usize {
    let Ok(entries) = fs::read_dir(backups_root) else { return 0 };
    let mut migrated = 0;
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        // 只剩 game.json 的空壳目录（实例删尽）→ 清掉
        if fs::read_dir(&dir).map(|mut d| {
            d.all(|e| e.map(|e| e.file_name().to_string_lossy() == GAME_INDEX_FILE).unwrap_or(false))
        }).unwrap_or(false) {
            let _ = fs::remove_file(dir.join(GAME_INDEX_FILE));
            let _ = fs::remove_dir(&dir);
            continue;
        }
        let Ok(id) = name.parse::<i64>() else {
            continue; // 非数字目录名：新布局，跳过
        };
        // 找一个可读的清单取标题（优先目录式实例，其次 zip 实例）
        let mut title: Option<String> = None;
        if let Ok(instances) = fs::read_dir(&dir) {
            let mut instance_names: Vec<String> = instances
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            instance_names.sort_by(|a, b| b.cmp(a)); // 新实例在前
            for name in &instance_names {
                let instance = dir.join(name);
                if instance.is_dir() {
                    if let Some(t) = read_manifest_in(&instance).ok().map(|m| m.title) {
                        title = Some(t);
                        break;
                    }
                } else if name.to_lowercase().ends_with(".zip") {
                    if let Some(t) = read_zip_manifest(&instance).ok().map(|m| m.title) {
                        title = Some(t);
                        break;
                    }
                }
            }
        }
        let Some(title) = title else {
            // 无任何可读清单：空目录删除（旧版误建），非空目录保留原名不动
            let is_empty = fs::read_dir(&dir).map(|mut d| d.next().is_none()).unwrap_or(false);
            if is_empty {
                let _ = fs::remove_dir(&dir);
            }
            continue;
        };
        let base = game_dir_name(&title);
        let mut target = backups_root.join(&base);
        // 碰撞处理：目标已存在且归属不同 → 追加 id 后缀
        if target != dir {
            if let Some(owner) = read_game_index(&target).map(|i| i.pcgw_id) {
                if owner != id {
                    target = backups_root.join(format!("{base} ({id})"));
                }
            } else if fs::read_dir(&target).map(|mut d| d.next().is_none()).unwrap_or(false) {
                // 目标为空目录，可直接占用
            } else if target.exists() {
                target = backups_root.join(format!("{base} ({id})"));
            }
        }
        if target == dir {
            write_game_index(&dir, &GameIndex {
                schema: GAME_INDEX_SCHEMA.into(),
                pcgw_id: id,
                title,
            })
            .ok();
            continue;
        }
        if fs::rename(&dir, &target).is_ok() {
            write_game_index(&target, &GameIndex {
                schema: GAME_INDEX_SCHEMA.into(),
                pcgw_id: id,
                title,
            })
            .ok();
            migrated += 1;
        }
    }
    migrated
}

/// 备份实例清单：每个实例（目录或 zip）内的自描述索引
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    /// 清单格式版本（当前 1；无该字段的文件按遗留格式语义处理）
    #[serde(default)]
    pub schema: u32,
    pub title: String,
    /// 中文标题（展示用，可空）
    pub title_zh: Option<String>,
    /// 繁体中文标题（展示用，可空；老清单没有这个字段）
    #[serde(default)]
    pub title_zh_hant: Option<String>,
    /// 创建时间戳，与实例目录名一致
    #[serde(default)]
    pub created_at: String,
    /// 备份内容的原始字节数（压缩包记录打包前的逻辑大小）
    pub size: Option<u64>,
    /// 固定标记：固定的实例不参与轮换清理
    pub pinned: Option<bool>,
    /// 用户自定义备注
    pub label: Option<String>,
    /// 内容条目
    pub entries: Vec<ManifestEntry>,
}

/// 清单内的单条内容：实例目录下的一个子目录 + 它的来源信息
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    /// 实例内的子目录名（按语义命名：存档/设置/注册表，重复追加序号）
    pub dir: String,
    /// 展示模板（占位符已回填；跨机器恢复时按新环境重解析）
    pub template: String,
    /// 存储形态：file | folder | reg
    pub store: String,
    pub install_folder: Option<String>,
    /// 语义类型：saves | config | reg
    pub content_type: Option<String>,
}

/// 遗留格式清单（backup_info.json）——只读翻译用
#[derive(Deserialize)]
struct LegacyManifest {
    title: String,
    #[serde(rename = "zh_CN")]
    zh_cn: Option<String>,
    backup_paths: Vec<LegacyEntry>,
    backup_size: Option<u64>,
    is_permanent: Option<bool>,
    custom_name: Option<String>,
}

#[derive(Deserialize)]
struct LegacyEntry {
    folder_name: String,
    template: String,
    #[serde(rename = "type")]
    kind: String,
    install_folder: Option<String>,
    content_kind: Option<String>,
}

impl From<LegacyManifest> for BackupManifest {
    fn from(l: LegacyManifest) -> Self {
        BackupManifest {
            schema: 0,
            title: l.title,
            title_zh: l.zh_cn,
            title_zh_hant: None,
            created_at: String::new(),
            size: l.backup_size,
            pinned: l.is_permanent,
            label: l.custom_name,
            entries: l
                .backup_paths
                .into_iter()
                .map(|e| ManifestEntry {
                    dir: e.folder_name,
                    template: e.template,
                    store: e.kind,
                    install_folder: e.install_folder,
                    content_type: e.content_kind,
                })
                .collect(),
        }
    }
}

/// 解析清单文本：先按当前格式，失败则按遗留格式翻译
fn parse_manifest(text: &str) -> Result<BackupManifest, String> {
    serde_json::from_str::<BackupManifest>(text)
        .or_else(|_| {
            serde_json::from_str::<LegacyManifest>(text).map(BackupManifest::from)
        })
        .map_err(|e| format!("清单解析失败: {e}"))
}

/// 从实例目录读取清单（当前格式优先，遗留文件兜底）
fn read_manifest_in(dir: &Path) -> Result<BackupManifest, String> {
    let text = fs::read_to_string(dir.join(MANIFEST_FILE))
        .or_else(|_| fs::read_to_string(dir.join(LEGACY_MANIFEST_FILE)))
        .map_err(|e| format!("备份缺少清单: {e}"))?;
    parse_manifest(&text)
}

/// 从 zip 包内读取清单（当前格式优先，遗留文件兜底）
fn read_zip_manifest(zip_path: &Path) -> Result<BackupManifest, String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut text = String::new();
    for name in [MANIFEST_FILE, LEGACY_MANIFEST_FILE] {
        if let Ok(mut entry) = archive.by_name(name) {
            std::io::Read::read_to_string(&mut entry, &mut text)
                .map_err(|e| e.to_string())?;
            return parse_manifest(&text);
        }
    }
    Err("备份包缺少清单".into())
}

/// 原子写 JSON：临时文件 + 改名，中途断电不损坏已有文件
fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(&tmp, json + "\n").map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// 实例固定状态：目录读清单，zip 读包内清单
fn instance_pinned(path: &Path, name: &str) -> bool {
    if path.is_dir() {
        read_manifest_in(path).ok().and_then(|m| m.pinned).unwrap_or(false)
    } else if name.to_lowercase().ends_with(".zip") {
        read_zip_manifest(path).ok().and_then(|m| m.pinned).unwrap_or(false)
    } else {
        false
    }
}

/// 恢复列表里的单个备份摘要（前端契约）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub date: String,
    pub title: String,
    #[serde(rename = "zhCn")]
    pub zh_cn: Option<String>,
    #[serde(rename = "zhHant", default)]
    pub zh_hant: Option<String>,
    pub backup_size: Option<u64>,
    pub backup_paths: Vec<BackupPathEntry>,
    pub is_permanent: bool,
    pub custom_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupPathEntry {
    pub folder_name: String,
    /// 展示模板：具体路径 + 已知占位符回填（跨机器恢复时按新环境重解析）
    pub template: String,
    #[serde(rename = "type")]
    pub kind: String, // "file" | "folder" | "reg"（存储形态）
    pub install_folder: Option<String>,
    /// 语义类型：saves=存档 config=设置 reg=注册表（旧备份无此字段）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_kind: Option<String>,
}

/// 一个游戏的全部备份（前端契约）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameBackups {
    pub pcgw_id: i64,
    pub latest_backup: String,
    pub title: String,
    #[serde(rename = "zhCn")]
    pub zh_cn: Option<String>,
    #[serde(rename = "zhHant")]
    pub zh_hant: Option<String>,
    pub backup_size: Option<u64>,
    pub backups: Vec<BackupSummary>,
}

impl From<(String, BackupManifest)> for BackupSummary {
    fn from((date, m): (String, BackupManifest)) -> Self {
        BackupSummary {
            date,
            title: m.title,
            zh_cn: m.title_zh,
            zh_hant: m.title_zh_hant,
            backup_size: m.size,
            backup_paths: m
                .entries
                .into_iter()
                .map(|e| BackupPathEntry {
                    folder_name: e.dir,
                    template: e.template,
                    kind: e.store,
                    install_folder: e.install_folder,
                    content_kind: e.content_type,
                })
                .collect(),
            is_permanent: m.pinned.unwrap_or(false),
            custom_name: m.label.unwrap_or_default(),
        }
    }
}

/// 递归复制目录，返回复制的字节数
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<u64, String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    let mut total = 0u64;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = dst.join(entry.file_name());
        let metadata = entry.metadata().map_err(|e| e.to_string())?;
        if metadata.is_dir() {
            total += copy_dir_recursive(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
            total += metadata.len();
        }
    }
    Ok(total)
}

/// 执行一次备份。resolved 是扫描得到的解析路径；返回备份实例路径（.zip 或目录）。
/// compress=true 时先在临时目录组装再打包成 .zip；全程同步（spawn_blocking 中调用）。
pub fn backup_game(
    entry: &crate::database::GameEntry,
    resolved: &[ResolvedPath],
    backups_root: &Path,
    max_backups: u32,
    compress: bool,
) -> Result<String, String> {
    // 游戏目录名用游戏标题；同名目录被其他游戏占用时追加 wiki id 后缀
    let base = game_dir_name(&entry.title);
    let mut game_dir = backups_root.join(&base);
    if game_dir.exists() {
        let owned = read_game_index(&game_dir).map(|i| i.pcgw_id) == Some(entry.pcgw_id);
        if !owned {
            let is_empty = fs::read_dir(&game_dir)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false);
            if !is_empty {
                game_dir = backups_root.join(format!("{base} ({})", entry.pcgw_id));
            }
        }
    }
    // 精确到秒：同一分钟内多次点击备份各存一份
    let mut instance_folder = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    // 防碰撞：同名（同秒连点）自动追加序号 _2、_3……
    let mut seq = 1u32;
    while game_dir.join(format!("{instance_folder}.zip")).exists()
        || game_dir.join(&instance_folder).exists()
    {
        seq += 1;
        instance_folder = format!(
            "{}_{seq}",
            Local::now().format("%Y-%m-%d_%H-%M-%S")
        );
    }
    // 组装阶段先放临时目录，完成后 zip 或改名就位，避免半成品出现在备份列表。
    // staging 名带纳秒与进程内序号：并行备份同一游戏（批量+自动备份同秒触发）不互删
    static STAGING_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let staging_seq = STAGING_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let staging_nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let instance_path = std::env::temp_dir().join(format!(
        "gm-staging-{}-{staging_nanos}-{staging_seq}",
        entry.pcgw_id
    ));
    let _ = fs::remove_dir_all(&instance_path);

    let mut manifest = BackupManifest {
        schema: 1,
        title: entry.title.clone(),
        title_zh: entry.zh_cn.clone(),
        title_zh_hant: entry.zh_hant.clone(),
        created_at: instance_folder.clone(),
        size: None,
        pinned: None,
        label: None,
        entries: Vec::new(),
    };

    let mut used_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for resolved_path in resolved.iter() {
        // 子目录按内容语义命名（存档/设置/注册表），同类多条自动加序号
        let base = match resolved_path.content_kind.as_str() {
            "config" => "设置",
            "reg" => "注册表",
            _ => "存档",
        };
        let mut seq = 1u32;
        let mut folder_name = base.to_string();
        while !used_names.insert(folder_name.clone()) {
            seq += 1;
            folder_name = format!("{base}{seq}");
        }
        let target = instance_path.join(&folder_name);
        fs::create_dir_all(&target).map_err(|e| e.to_string())?;

        let store = match resolved_path.kind {
            crate::resolve::ResolvedKind::Registry => {
                // 注册表导出仅 Windows（reg.exe）；非 Windows 不应出现 reg 候选
                #[cfg(target_os = "windows")]
                {
                    let reg_file = target.join("registry_backup.reg");
                    let output = std::process::Command::new("reg")
                        .args(["export", &resolved_path.resolved])
                        .arg(&reg_file)
                        .arg("/y")
                        .output()
                        .map_err(|e| e.to_string())?;
                    if !output.status.success() {
                        return Err(format!(
                            "reg export 失败: {}",
                            String::from_utf8_lossy(&output.stderr)
                        ));
                    }
                    "reg"
                }
                #[cfg(not(target_os = "windows"))]
                {
                    return Err("注册表备份仅支持 Windows".to_string());
                }
            }
            crate::resolve::ResolvedKind::File => {
                let source = Path::new(&resolved_path.resolved);
                let metadata = fs::metadata(source).map_err(|e| e.to_string())?;
                if metadata.is_dir() {
                    copy_dir_recursive(source, &target)?;
                    "folder"
                } else {
                    let file_name = source.file_name().ok_or("路径缺少文件名")?;
                    fs::copy(source, target.join(file_name)).map_err(|e| e.to_string())?;
                    "file"
                }
            }
        };

        manifest.entries.push(ManifestEntry {
            dir: folder_name,
            template: resolved_path.final_template.clone(),
            store: store.to_string(),
            install_folder: entry.install_folder.clone(),
            content_type: Some(resolved_path.content_kind.clone()),
        });
    }

    // 先量大小再写清单，保证记录值与之后 walk 的结果一致
    let (size, _) = crate::scanner::walk_size(&instance_path);
    manifest.size = Some(size);

    write_json_atomic(&instance_path.join(MANIFEST_FILE), &manifest)?;

    fs::create_dir_all(&game_dir).map_err(|e| e.to_string())?;
    write_game_index(&game_dir, &GameIndex {
        schema: GAME_INDEX_SCHEMA.into(),
        pcgw_id: entry.pcgw_id,
        title: entry.title.clone(),
    })?;
    let final_path = if compress {
        let zip_path = game_dir.join(format!("{instance_folder}.zip"));
        zip_dir(&instance_path, &zip_path)?;
        fs::remove_dir_all(&instance_path).map_err(|e| e.to_string())?;
        zip_path
    } else {
        let dest = game_dir.join(&instance_folder);
        // 临时目录通常同盘可改名；跨盘退化为复制
        if fs::rename(&instance_path, &dest).is_err() {
            copy_dir_recursive(&instance_path, &dest)?;
            fs::remove_dir_all(&instance_path).map_err(|e| e.to_string())?;
        }
        dest
    };

    // 跨目录轮换：游戏改过名时可能有多个目录，必须合起来算保留数
    let dirs = game_dirs_for(backups_root, entry.pcgw_id);
    apply_rotation_multi(&dirs, max_backups)?;
    Ok(final_path.to_string_lossy().to_string())
}

/// zip 打包时的单文件写入器（writer + 相对路径 + 源文件）
type ZipSink<'a> = dyn FnMut(&mut zip::ZipWriter<fs::File>, String, &Path) -> Result<(), String> + 'a;

/// 把目录打包为 zip（相对路径一律用 / 分隔）
fn zip_dir(source: &Path, dest_zip: &Path) -> Result<(), String> {
    let file = fs::File::create(dest_zip).map_err(|e| e.to_string())?;
    let mut writer = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut collect = |writer: &mut zip::ZipWriter<fs::File>,
                       rel: String,
                       full: &Path|
     -> Result<(), String> {
        writer
            .start_file(rel, options)
            .map_err(|e| e.to_string())?;
        let bytes = fs::read(full).map_err(|e| e.to_string())?;
        std::io::Write::write_all(writer, &bytes).map_err(|e| e.to_string())
    };

    fn walk(
        dir: &Path,
        base: &Path,
        writer: &mut zip::ZipWriter<fs::File>,
        collect: &mut ZipSink,
    ) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let full = entry.path();
            let rel = full
                .strip_prefix(base)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if full.is_dir() {
                walk(&full, base, writer, collect)?;
            } else {
                collect(writer, rel, &full)?;
            }
        }
        Ok(())
    }

    walk(source, source, &mut writer, &mut collect)?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// 解压整个 zip 到目标目录
pub fn extract_zip(zip_path: &Path, dest_dir: &Path) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    fs::create_dir_all(dest_dir).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = dest_dir.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut out_file = fs::File::create(&out).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out_file).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// 重写 zip 包：把全部条目按给定字节内容重新打包（Deflated）
fn rewrite_zip(zip_path: &Path, entries: &[(String, Vec<u8>)]) -> Result<(), String> {
    let tmp = zip_path.with_extension("zip.tmp");
    let file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut writer = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        writer.start_file(name.as_str(), options).map_err(|e| e.to_string())?;
        std::io::Write::write_all(&mut writer, bytes).map_err(|e| e.to_string())?;
    }
    writer.finish().map_err(|e| e.to_string())?;
    fs::rename(&tmp, zip_path).map_err(|e| e.to_string())
}

/// 修改 zip 内清单的固定标记；遗留清单升级为 manifest.json（旧条目不再保留）
fn set_zip_pinned(zip_path: &Path, pinned: bool) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(|e| e.to_string())?;
        entries.push((name, bytes));
    }
    drop(archive);

    let manifest: BackupManifest = {
        let idx = entries
            .iter()
            .position(|(n, _)| n == MANIFEST_FILE || n == LEGACY_MANIFEST_FILE)
            .ok_or("备份包缺少清单")?;
        parse_manifest(&String::from_utf8_lossy(&entries[idx].1))?
    };
    let mut updated = manifest;
    updated.pinned = Some(pinned);
    let new_json = serde_json::to_string_pretty(&updated).map_err(|e| e.to_string())?;

    // 重组条目：去掉遗留清单，统一以 manifest.json 落盘
    let mut out: Vec<(String, Vec<u8>)> = Vec::with_capacity(entries.len());
    out.push((MANIFEST_FILE.to_string(), new_json.into_bytes()));
    for (name, bytes) in &entries {
        if name != MANIFEST_FILE && name != LEGACY_MANIFEST_FILE {
            out.push((name.clone(), bytes.clone()));
        }
    }
    rewrite_zip(zip_path, &out)
}

/// 轮换清理：非固定实例（目录或 zip）超出上限时删除最旧的
/// 跨目录轮换：把**同一个游戏所有备份目录**里的实例合起来算保留数。
///
/// 为什么需要：备份目录名来自游戏标题（`game_dir_name`），标题改过之后新备份
/// 会进新目录、旧目录仍留在磁盘上。只对当前目录做轮换的话，旧目录里的备份
/// 永远不会被清理，实际保留数 = 目录数 × max_backups，磁盘只增不减。
///
/// 实例名是 `2026-11-01_22-16-30` 这种时间戳，按名字排序即按时间排序；
/// 目录实例和 zip 实例都要能删（老实现只 `remove_dir_all`，遇到 zip 会失败）。
pub fn apply_rotation_multi(dirs: &[PathBuf], max_backups: u32) -> Result<(), String> {
    let mut all: Vec<(String, PathBuf)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = dir.join(&name);
            let is_instance_dir = path.is_dir();
            let is_zip = name.to_lowercase().ends_with(".zip");
            if !is_instance_dir && !is_zip {
                continue;
            }
            // 无清单或清单缺失 pinned 的实例视为可清理
            if !instance_pinned(&path, &name) {
                all.push((name, dir.clone()));
            }
        }
    }
    all.sort_by(|a, b| a.0.cmp(&b.0));
    if all.len() <= max_backups as usize {
        return Ok(());
    }
    for (name, dir) in &all[..all.len() - max_backups as usize] {
        let path = dir.join(name);
        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.map_err(|e| format!("清理旧备份失败: {e}"))?;
    }
    Ok(())
}

pub fn apply_rotation(game_backup_path: &Path, max_backups: u32) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(game_backup_path) else {
        return Ok(());
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| {
            let p = game_backup_path.join(n);
            p.is_dir() || n.to_lowercase().ends_with(".zip")
        })
        .collect();
    names.sort();

    let mut removable: Vec<String> = Vec::new();
    for name in &names {
        let path = game_backup_path.join(name);
        // 无清单或清单缺失字段的实例视为可清理
        if !instance_pinned(&path, name) {
            removable.push(name.clone());
        }
    }

    if removable.len() > max_backups as usize {
        let to_delete = &removable[..removable.len() - max_backups as usize];
        for folder in to_delete {
            fs::remove_dir_all(game_backup_path.join(folder))
                .map_err(|e| format!("清理旧备份失败: {e}"))?;
        }
    }
    Ok(())
}

/// 枚举一个游戏的全部备份（zip 与目录实例混合，按时间戳倒序）
pub fn list_backups(backups_root: &Path, pcgw_id: i64) -> Result<Option<GameBackups>, String> {
    let dirs = game_dirs_for(backups_root, pcgw_id);
    if dirs.is_empty() {
        return Ok(None);
    }

    // 同一游戏可能有多个目录（title 改过之后新备份进新目录、旧目录还在）。
    // 全部扫进来，按 date 去重——同一天只保留先扫到的那个。
    let mut found: Vec<(BackupSummary, PathBuf)> = Vec::new();
    for game_backup_path in &dirs {
        for entry in fs::read_dir(game_backup_path).map_err(|e| e.to_string())?.flatten() {
            let raw_name = entry.file_name().to_string_lossy().to_string();
            let (date, summary) = if entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                // 目录式备份（未压缩）
                let manifest = match read_manifest_in(&entry.path()) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                (raw_name, BackupSummary::from((String::new(), manifest)))
            } else if raw_name.to_lowercase().ends_with(".zip") {
                // zip 压缩备份：磁盘占用即文件大小，清单里记录的是打包前逻辑大小
                let Some(stem) = raw_name.strip_suffix(".zip") else { continue };
                let manifest = match read_zip_manifest(&entry.path()) {
                    Ok(m) => m,
                    Err(_) => continue,
                };
                (stem.to_string(), BackupSummary::from((String::new(), manifest)))
            } else {
                continue;
            };
            let mut summary = summary;
            summary.date = date;
            if found.iter().any(|(s, _)| s.date == summary.date) {
                continue;
            }
            found.push((summary, game_backup_path.clone()));
        }
    }

    if found.is_empty() {
        return Ok(None);
    }
    // 实例名即时间戳，字典序即时间序
    found.sort_by(|a, b| b.0.date.cmp(&a.0.date));
    let latest_path = found[0].1.join(&found[0].0.date);
    let mut backups: Vec<BackupSummary> = found.into_iter().map(|(s, _)| s).collect();

    // 列表只展示最新实例的实测大小；缺失时补量一次并回写清单（仅目录式实例）
    if backups[0].backup_size.is_none() && latest_path.is_dir() {
        let (size, _) = crate::scanner::walk_size(&latest_path);
        if let Some(summary) = backups.first_mut() {
            summary.backup_size = Some(size);
        }
        let mut manifest = read_manifest_in(&latest_path)?;
        manifest.size = Some(size);
        write_json_atomic(&latest_path.join(MANIFEST_FILE), &manifest)?;
        // 遗留清单升级：manifest.json 已就位，旧文件移除
        let _ = fs::remove_file(latest_path.join(LEGACY_MANIFEST_FILE));
    }

    Ok(Some(GameBackups {
        pcgw_id,
        latest_backup: format_backup_date(&backups[0].date),
        title: backups[0].title.clone(),
        zh_cn: backups[0].zh_cn.clone(),
        zh_hant: backups[0].zh_hant.clone(),
        backup_size: backups[0].backup_size,
        backups,
    }))
}

/// "2024-09-08_15-23" → "2024/09/08 15:23"；"2024-09-08_15-23-45" → "2024/09/08 15:23:45"
pub fn format_backup_date(date: &str) -> String {
    let (day, time) = date.split_once('_').unwrap_or((date, ""));
    format!(
        "{} {}",
        day.replace('-', "/"),
        time.replace('-', ":")
    )
    .trim_end()
    .to_string()
}

/// 枚举备份根目录下所有带备份的游戏（按标题字母序）。
/// 目录归属由 game.json 识别；无索引的目录（未迁移的旧布局）按数字 id 兜底。
pub fn list_all_backups(backups_root: &Path) -> Result<Vec<GameBackups>, String> {
    let mut games = Vec::new();
    if !backups_root.is_dir() {
        return Ok(games);
    }
    let mut seen_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();
    for entry in fs::read_dir(backups_root).map_err(|e| e.to_string())?.flatten() {
        if !entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
            continue;
        }
        let dir = entry.path();
        let id = match read_game_index(&dir) {
            Some(index) => index.pcgw_id,
            None => match dir.file_name().and_then(|n| n.to_string_lossy().parse::<i64>().ok()) {
                Some(id) => id, // 旧布局数字目录
                None => continue,
            },
        };
        // 同一 pcgw_id 可能有多个目录（改名遗留），只处理一次
        if !seen_ids.insert(id) {
            continue;
        }
        if let Some(game) = list_backups(backups_root, id)? {
            games.push(game);
        }
    }
    games.sort_by_key(|a| a.title.to_lowercase());
    Ok(games)
}

/// 各游戏的备份数量（恢复列表/右键菜单判断"是否已备份"用）
pub fn backup_counts(backups_root: &Path) -> std::collections::HashMap<i64, u32> {
    let mut out = std::collections::HashMap::new();
    let Ok(entries) = fs::read_dir(backups_root) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(id) = read_game_index(&dir)
            .map(|i| i.pcgw_id)
            .or_else(|| dir.file_name().and_then(|n| n.to_string_lossy().parse::<i64>().ok()))
        else {
            continue;
        };
        let count = fs::read_dir(&dir)
            .map(|its| {
                its.flatten()
                    .filter(|it| {
                        it.path().is_dir() || it.file_name().to_string_lossy().to_lowercase().ends_with(".zip")
                    })
                    .count() as u32
            })
            .unwrap_or(0);
        *out.entry(id).or_insert(0) += count;
    }
    out
}

/// 游戏目录里只剩归属索引（game.json）时删掉整个目录——
/// 最后一份备份被删除后不留空壳
fn remove_game_dir_if_empty(game_dir: &Path) {
    let Ok(mut entries) = fs::read_dir(game_dir) else { return };
    let only_index = entries
        .all(|e| match e {
            Ok(e) => e.file_name().to_string_lossy() == GAME_INDEX_FILE,
            Err(_) => false,
        });
    if only_index {
        let _ = fs::remove_file(game_dir.join(GAME_INDEX_FILE));
        let _ = fs::remove_dir(game_dir);
    }
}

/// 删除指定备份（目录或 zip）
pub fn delete_backup(backups_root: &Path, pcgw_id: i64, date: &str) -> Result<(), String> {
    let folder = validate_date(date)?;
    let base = game_dir_for(backups_root, pcgw_id)
        .ok_or_else(|| format!("备份不存在: {date}"))?;
    let dir = base.join(folder);
    let zip = base.join(format!("{folder}.zip"));
    if dir.is_dir() {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    } else if zip.is_file() {
        fs::remove_file(&zip).map_err(|e| e.to_string())?;
    }
    remove_game_dir_if_empty(&base);
    Ok(())
}

/// 设置固定 / 取消固定（目录改清单并迁移；zip 重写包内清单）
pub fn set_backup_permanent(
    backups_root: &Path,
    pcgw_id: i64,
    date: &str,
    permanent: bool,
) -> Result<(), String> {
    let folder = validate_date(date)?;
    let base = game_dir_for(backups_root, pcgw_id)
        .ok_or_else(|| format!("备份不存在: {date}"))?;
    let dir = base.join(folder);
    let zip = base.join(format!("{folder}.zip"));
    if dir.is_dir() {
        let mut manifest = read_manifest_in(&dir)?;
        manifest.pinned = Some(permanent);
        write_json_atomic(&dir.join(MANIFEST_FILE), &manifest)?;
        // 遗留清单升级
        let _ = fs::remove_file(dir.join(LEGACY_MANIFEST_FILE));
        Ok(())
    } else if zip.is_file() {
        set_zip_pinned(&zip, permanent)
    } else {
        Err(format!("备份不存在: {date}"))
    }
}

/// 防路径穿越：date 只允许时间戳格式（数字、连字符、下划线）
pub fn validate_date(date: &str) -> Result<&str, String> {
    let ok = !date.is_empty()
        && date
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-' || c == '_')
        && !date.contains("..");
    if ok { Ok(date) } else { Err(format!("非法的备份日期: {date}")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::GameEntry;
    use crate::database::SaveLocations;
    use crate::resolve::ResolvedKind;

    fn test_entry() -> GameEntry {
        GameEntry {
            title: "CoolGame".into(),
            pcgw_id: 42,
            install_folder: Some("CoolGame".into()),
            steam_id: None,
            gog_id: None,
            platforms: vec![],
            zh_cn: Some("酷游戏".into()),
            zh_hant: None,
            paths: SaveLocations::default(),
        }
    }

    #[test]
    fn backup_and_list_roundtrip() {
        let root = std::env::temp_dir().join(format!("gm-backup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.sav"), vec![b'x'; 128]).unwrap();

        let resolved = vec![ResolvedPath {
            template: "{{p|game}}\\saves".into(),
            final_template: "{{p|game}}\\saves".into(),
            resolved: src.join("a.sav").to_string_lossy().to_string(),
            kind: ResolvedKind::File,
            content_kind: "saves".into(),
        }];

        let backups_root = root.join("backups");
        let instance = backup_game(&test_entry(), &resolved, &backups_root, 3, false).unwrap();
        assert!(instance.contains("CoolGame"), "实例路径应含游戏名: {instance}");

        // 目录结构：backups/CoolGame/<date>/存档/a.sav + manifest.json，game.json 声明归属
        assert_eq!(
            read_game_index(&backups_root.join("CoolGame")).map(|i| i.pcgw_id),
            Some(42)
        );
        let listed = list_backups(&backups_root, 42).unwrap().unwrap();
        assert_eq!(listed.backups.len(), 1);
        assert_eq!(listed.backups[0].title, "CoolGame");
        assert_eq!(listed.backups[0].zh_cn.as_deref(), Some("酷游戏"));
        assert_eq!(listed.backups[0].backup_paths.len(), 1);
        assert_eq!(listed.backups[0].backup_paths[0].kind, "file");
        assert!(listed.backups[0].backup_size.unwrap() >= 128);

        // 压缩备份：同样的内容打包为 zip，列表/固定/删除全兼容
        let instance_zip =
            backup_game(&test_entry(), &resolved, &backups_root, 3, true).unwrap();
        assert!(instance_zip.ends_with(".zip"));
        let listed = list_backups(&backups_root, 42).unwrap().unwrap();
        assert_eq!(listed.backups.len(), 2);
        assert!(listed.backups[0].backup_size.unwrap() >= 128);
        set_backup_permanent(&backups_root, 42, &listed.backups[0].date, true).unwrap();
        let reloaded = list_backups(&backups_root, 42).unwrap().unwrap();
        assert!(reloaded.backups[0].is_permanent);
        delete_backup(&backups_root, 42, &reloaded.backups[0].date).unwrap();
        assert_eq!(list_backups(&backups_root, 42).unwrap().unwrap().backups.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    /// 同一 pcgw_id 对应多个目录时（游戏英文名改过，旧目录留在磁盘上），
    /// 列表必须把两个目录的备份都算进来，否则用户会觉得改名前的备份丢了。
    #[test]
    fn renamed_game_dir_merges_by_pcgw_id() {
        let root = std::env::temp_dir().join(format!("gm-rename-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.sav"), vec![b'x'; 64]).unwrap();

        let resolved = vec![ResolvedPath {
            template: "{{p|game}}\\saves".into(),
            final_template: "{{p|game}}\\saves".into(),
            resolved: src.join("a.sav").to_string_lossy().to_string(),
            kind: ResolvedKind::File,
            content_kind: "saves".into(),
        }];
        let backups_root = root.join("backups");

        // 两个备份实例（时间戳精确到秒，错开一秒保证目录名不同）
        backup_game(&test_entry(), &resolved, &backups_root, 5, false).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        backup_game(&test_entry(), &resolved, &backups_root, 5, false).unwrap();

        // 模拟改名：把较早的实例挪到另一个目录，并让新目录同样声明 pcgw_id=42
        let listed = list_backups(&backups_root, 42).unwrap().unwrap();
        assert_eq!(listed.backups.len(), 2);
        let older = listed.backups[1].date.clone();
        let renamed_dir = backups_root.join("CoolGame Renamed");
        fs::create_dir_all(&renamed_dir).unwrap();
        write_game_index(
            &renamed_dir,
            &GameIndex {
                schema: GAME_INDEX_SCHEMA.into(),
                pcgw_id: 42,
                title: "CoolGame Renamed".into(),
            },
        )
        .unwrap();
        fs::rename(
            backups_root.join("CoolGame").join(&older),
            renamed_dir.join(&older),
        )
        .unwrap();

        // 单游戏列表：两个目录的实例都要出现
        let merged = list_backups(&backups_root, 42).unwrap().unwrap();
        assert_eq!(merged.backups.len(), 2, "改名前后两个目录的备份都要列出");

        // 总列表：同一 pcgw_id 只能出现一次
        let all = list_all_backups(&backups_root).unwrap();
        assert_eq!(all.len(), 1, "同一 pcgw_id 不应因为多目录而重复");
        assert_eq!(all[0].backups.len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rotation_spans_all_dirs_of_same_game() {
        // 场景：游戏改过名 → 备份分在两个目录（Old Name / New Name），都属于 pcgw_id 42。
        // 轮换必须把两个目录的实例合起来算，否则旧目录里的备份永远不清理。
        let root = std::env::temp_dir().join(format!("gm-rotation-multi-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let old_dir = root.join("Old Name");
        let new_dir = root.join("New Name");
        let cases: [(&std::path::PathBuf, &[&str]); 2] = [
            (&old_dir, &["2024-01-01_10-00", "2024-01-02_10-00", "2024-01-03_10-00"]),
            (&new_dir, &["2024-01-04_10-00", "2024-01-05_10-00"]),
        ];
        for (dir, dates) in cases {
            fs::create_dir_all(dir).unwrap();
            write_game_index(
                dir,
                &GameIndex { schema: "gm.game-index/1".into(), pcgw_id: 42, title: "CoolGame".into() },
            )
            .unwrap();
            for date in dates {
                let inst = dir.join(date);
                fs::create_dir_all(&inst).unwrap();
                write_json_atomic(&inst.join(MANIFEST_FILE), &BackupManifest {
                    schema: 1,
                    title: "CoolGame".into(),
                    title_zh: None,
                    title_zh_hant: None,
                    created_at: (*date).to_string(),
                    size: Some(1),
                    pinned: None,
                    label: None,
                    entries: vec![],
                })
                .unwrap();
            }
        }

        let dirs = game_dirs_for(&root, 42);
        assert_eq!(dirs.len(), 2, "两个目录都应归属同一个游戏");
        apply_rotation_multi(&dirs, 3).unwrap();

        let total: usize = [&old_dir, &new_dir]
            .iter()
            .map(|d| {
                fs::read_dir(d)
                    .unwrap()
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .count()
            })
            .sum();
        assert_eq!(total, 3, "两个目录合计只应保留 3 份");
        // 保留的是最新三份，旧目录里最老的两份被清掉
        assert!(new_dir.join("2024-01-05_10-00").is_dir());
        assert!(!old_dir.join("2024-01-01_10-00").is_dir());
        assert!(!old_dir.join("2024-01-02_10-00").is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rotation_removes_oldest_non_permanent() {
        // 直接手工构造备份目录来测轮换，不依赖时间戳推进
        let root = std::env::temp_dir().join(format!("gm-rotation-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let game_dir = root.join("42");
        for date in ["2024-01-01_10-00", "2024-01-02_10-00", "2024-01-03_10-00", "2024-01-04_10-00"] {
            let dir = game_dir.join(date);
            fs::create_dir_all(&dir).unwrap();
            write_json_atomic(&dir.join(MANIFEST_FILE), &BackupManifest {
                schema: 1,
                title: "CoolGame".into(),
                title_zh: None,
                title_zh_hant: None,
                created_at: date.into(),
                size: Some(1),
                pinned: None,
                label: None,
                entries: vec![],
            })
            .unwrap();
        }
        apply_rotation(&game_dir, 2).unwrap();
        let remaining: Vec<String> = fs::read_dir(&game_dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.contains(&"2024-01-03_10-00".to_string()));
        assert!(remaining.contains(&"2024-01-04_10-00".to_string()));

        // 固定实例不计入上限，也不会被清理
        let mut manifest = read_manifest_in(&game_dir.join("2024-01-03_10-00")).unwrap();
        manifest.pinned = Some(true);
        write_json_atomic(&game_dir.join("2024-01-03_10-00").join(MANIFEST_FILE), &manifest).unwrap();
        apply_rotation(&game_dir, 1).unwrap();
        let remaining: Vec<String> = fs::read_dir(&game_dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(remaining.contains(&"2024-01-03_10-00".to_string())); // 固定保留
        assert!(remaining.contains(&"2024-01-04_10-00".to_string()));
        assert_eq!(remaining.len(), 2);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_manifest_still_readable_and_upgrades() {
        // 早期实例（backup_info.json / snake_case 键名）只读兼容，固定操作时升级
        let root = std::env::temp_dir().join(format!("gm-legacy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("42").join("2024-02-03_09-30");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(LEGACY_MANIFEST_FILE),
            r#"{
  "title": "OldGame",
  "zh_CN": "老游戏",
  "backup_paths": [
    {"folder_name": "存档", "template": "{{p|appdata}}\\Old", "type": "folder",
     "install_folder": null, "content_kind": "saves"}
  ],
  "backup_size": 777,
  "is_permanent": false,
  "custom_name": "备注甲"
}"#,
        )
        .unwrap();

        // 数字目录（旧布局）经迁移后按游戏名命名并写入 game.json
        assert_eq!(migrate_numeric_game_dirs(&root), 1);
        assert!(root.join("OldGame").is_dir());
        assert!(!root.join("42").exists());
        assert_eq!(
            read_game_index(&root.join("OldGame")).map(|i| i.pcgw_id),
            Some(42)
        );

        let listed = list_backups(&root, 42).unwrap().unwrap();
        assert_eq!(listed.backups.len(), 1);
        assert_eq!(listed.backups[0].title, "OldGame");
        assert_eq!(listed.backups[0].zh_cn.as_deref(), Some("老游戏"));
        assert_eq!(listed.backups[0].backup_size, Some(777));
        assert_eq!(listed.backups[0].custom_name, "备注甲");
        assert_eq!(listed.backups[0].backup_paths[0].kind, "folder");
        assert_eq!(listed.backups[0].backup_paths[0].content_kind.as_deref(), Some("saves"));

        // 固定 → 清单升级为 manifest.json，旧文件移除（目录已被迁移改名为 OldGame）
        let migrated_dir = root.join("OldGame").join("2024-02-03_09-30");
        set_backup_permanent(&root, 42, "2024-02-03_09-30", true).unwrap();
        assert!(migrated_dir.join(MANIFEST_FILE).exists());
        assert!(!migrated_dir.join(LEGACY_MANIFEST_FILE).exists());
        let reloaded = list_backups(&root, 42).unwrap().unwrap();
        assert!(reloaded.backups[0].is_permanent);
        assert_eq!(reloaded.backups[0].backup_paths.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn game_dir_naming_claim_conflict_and_counts() {
        let root = std::env::temp_dir().join(format!("gm-naming-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        // 空目录可被同名游戏认领；被其他游戏占用时追加 id 后缀
        let claimed = root.join("CoolGame");
        fs::create_dir_all(&claimed).unwrap();
        let e1 = test_entry(); // pcgw_id 42, title CoolGame
        let mut e2 = test_entry();
        e2.pcgw_id = 99;
        e2.title = "CoolGame: Remastered".into();

        let resolved = vec![ResolvedPath {
            template: "{{p|game}}\\saves".into(),
            final_template: "{{p|game}}\\saves".into(),
            resolved: root.join("src").join("a.sav").to_string_lossy().to_string(),
            kind: ResolvedKind::File,
            content_kind: "saves".into(),
        }];
        let src = root.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.sav"), b"x").unwrap();

        // 空的 CoolGame 目录被 42 认领
        let p1 = backup_game(&e1, &resolved, &root, 3, false).unwrap();
        assert!(p1.contains("CoolGame") && !p1.contains("("));
        // 另一游戏标题不同 → 各自目录（sanitize 后冒号被去掉）
        let p2 = backup_game(&e2, &resolved, &root, 3, false).unwrap();
        assert!(p2.contains("CoolGame Remastered"), "p2={p2}");
        // 模拟同名冲突：把 99 号游戏标题改成与 42 相同 → 加 (99) 后缀
        let mut e3 = test_entry();
        e3.pcgw_id = 99;
        let p3 = backup_game(&e3, &resolved, &root, 3, false).unwrap();
        assert!(p3.contains("CoolGame (99)"), "碰撞应加 id 后缀: {p3}");

        // 备份计数按归属统计
        let counts = backup_counts(&root);
        assert_eq!(counts.get(&42), Some(&1));
        assert_eq!(counts.get(&99), Some(&2));

        // 删除/固定经由 game_dir_for 解析
        let listed = list_backups(&root, 99).unwrap().unwrap();
        let target = &listed.backups[0].date;
        delete_backup(&root, 99, target).unwrap();
        assert_eq!(backup_counts(&root).get(&99), Some(&1));
        // 删掉最后一个实例 → 游戏目录（含 game.json）一并清除，不留空壳
        let listed = list_backups(&root, 99).unwrap().unwrap();
        let target = &listed.backups[0].date;
        delete_backup(&root, 99, target).unwrap();
        assert!(list_backups(&root, 99).unwrap().is_none());
        assert!(!root.join("CoolGame (99)").exists());
        assert_eq!(backup_counts(&root).get(&99), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migration_skips_empty_dirs_and_keeps_unreadable() {
        let root = std::env::temp_dir().join(format!("gm-mig2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        // 误建空目录 → 删除
        fs::create_dir_all(root.join("77")).unwrap();
        // 无清单的非空目录 → 保留原名不动
        let junk = root.join("88");
        fs::create_dir_all(junk.join("random")).unwrap();
        assert_eq!(migrate_numeric_game_dirs(&root), 0);
        assert!(!root.join("77").exists());
        assert!(junk.is_dir());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn date_validation_blocks_traversal() {
        assert!(validate_date("2024-01-01_10-00").is_ok());
        assert!(validate_date("../evil").is_err());
        assert!(validate_date("").is_err());
    }
}
