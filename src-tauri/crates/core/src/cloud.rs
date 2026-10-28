//! 云备份打包：把一个备份实例打成一个 zip，用于上传到 WebDAV。
//!
//! 设计要点：
//! - **默认 deflate 压缩**（实测：65 KB 的 RPG Maker 备份压到 49 KB，省 25%，
//!   只多花约 3 ms）。存档文件看着像二进制，实际多为 JSON 文本，压缩率不低。
//!   大存档的收益会放大到几十 MB，而网络传输远比这 3 ms 贵。
//! - **可选排除注册表**：注册表项跨机器恢复多半不适用，且可能把别的机器设置带过来。
//!   排除时会同步从 zip 内的 manifest.json 里删掉对应条目，否则恢复时会找不到文件。
//! - 产物是**可直接使用的备份**：解压到 Backups/<游戏名>/ 下即可被应用识别。

use crate::backup::{BackupManifest, ManifestEntry};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleInfo {
    /// 建议的云端文件名（含 .zip）
    pub file_name: String,
    /// zip 字节数
    pub size: u64,
    /// 原始备份字节数
    pub raw_size: u64,
    /// 游戏英文名
    pub title: String,
    /// 游戏中文名
    pub title_zh: Option<String>,
    /// 备份时间戳（与实例目录名一致）
    pub created_at: String,
    /// 实际打包的条目数（排除注册表后）
    pub entries: usize,
    /// 因排除注册表而跳过的条目数
    pub skipped_registry: usize,
}

/// 已经压缩过的扩展名：再 deflate 只会白烧 CPU（有时还会变大）
const PRECOMPRESSED: &[&str] = &[
    "zip", "rar", "7z", "gz", "bz2", "xz", "zst",
    "png", "jpg", "jpeg", "webp", "gif", "avif",
    "mp3", "ogg", "opus", "flac", "m4a",
    "mp4", "webm", "mkv", "avi", "mov",
];

/// 按扩展名挑选压缩方式：文本/未知类型 deflate，已压缩格式直接存。
fn opts_for(path: &Path) -> SimpleFileOptions {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let method = if PRECOMPRESSED.contains(&ext.as_str()) {
        CompressionMethod::Stored
    } else {
        CompressionMethod::Deflated
    };
    SimpleFileOptions::default().compression_method(method)
}

/// 文件名清洗：去掉 Windows 与常见 WebDAV 不友好字符
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "unnamed".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_registry(entry: &ManifestEntry) -> bool {
    entry.store == "reg" || entry.content_type.as_deref() == Some("reg")
}

/// 把备份实例目录打成 zip。
///
/// `instance_dir` 形如 `<Backups>/<游戏名>/<时间戳>/`；`out_zip` 为输出路径。
/// 打包后 zip 内顶层是 `manifest.json` + 各内容子目录。
pub fn build_bundle(
    instance_dir: &Path,
    backup_title: &str,
    created_at: &str,
    out_zip: &Path,
    exclude_registry: bool,
) -> Result<BundleInfo, String> {
    let manifest_path = instance_dir.join("manifest.json");
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读取清单失败: {e}"))?;
    let manifest: BackupManifest =
        serde_json::from_str(&manifest_text).map_err(|e| format!("解析清单失败: {e}"))?;

    let (kept, skipped): (Vec<ManifestEntry>, Vec<ManifestEntry>) = manifest
        .entries
        .iter()
        .cloned()
        .partition(|e| !(exclude_registry && is_registry(e)));

    // 重写清单：只保留真正打进包里的条目
    let mut out_manifest = manifest.clone();
    out_manifest.entries = kept.clone();
    let manifest_json = serde_json::to_string_pretty(&out_manifest)
        .map_err(|e| format!("序列化清单失败: {e}"))?;

    if let Some(parent) = out_zip.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = File::create(out_zip).map_err(|e| format!("创建压缩包失败: {e}"))?;
    let mut zip = ZipWriter::new(file);

    zip.start_file(
        "manifest.json",
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )
    .map_err(|e| e.to_string())?;
    zip.write_all(manifest_json.as_bytes()).map_err(|e| e.to_string())?;

    // 游戏目录的归属索引（含 pcgwId + title）。没有它，下载回来就无法确定属于哪个游戏。
    if let Some(parent) = instance_dir.parent() {
        let idx = parent.join("game.json");
        if idx.exists() {
            if let Ok(text) = std::fs::read(&idx) {
                zip.start_file(
                    "game.json",
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .map_err(|e| e.to_string())?;
                zip.write_all(&text).map_err(|e| e.to_string())?;
            }
        }
    }

    let mut raw_size = 0u64;
    let mut buf = vec![0u8; 64 * 1024];

    for entry in &kept {
        let src = instance_dir.join(&entry.dir);
        if entry.store == "file" {
            if !src.exists() {
                continue;
            }
            let name = format!("{}/{}", entry.dir, file_name_of(&src));
            let mut f = File::open(&src).map_err(|e| e.to_string())?;
            zip.start_file(name, opts_for(&src)).map_err(|e| e.to_string())?;
            loop {
                let n = f.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                raw_size += n as u64;
                zip.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            }
        } else if src.is_dir() {
            for item in walk(&src) {
                let rel = item
                    .strip_prefix(instance_dir)
                    .unwrap_or(&item)
                    .to_string_lossy()
                    .replace('\\', "/");
                if item.is_dir() {
                    zip.add_directory(format!("{rel}/"), SimpleFileOptions::default())
                        .map_err(|e| e.to_string())?;
                    continue;
                }
                let mut f = File::open(&item).map_err(|e| e.to_string())?;
                zip.start_file(rel, opts_for(&item)).map_err(|e| e.to_string())?;
                loop {
                    let n = f.read(&mut buf).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    raw_size += n as u64;
                    zip.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                }
            }
        }
    }

    zip.finish().map_err(|e| format!("写入压缩包失败: {e}"))?;
    let size = std::fs::metadata(out_zip).map(|m| m.len()).unwrap_or(0);

    Ok(BundleInfo {
        file_name: format!("{}__{}.zip", safe_name(backup_title), safe_name(created_at)),
        size,
        raw_size,
        title: backup_title.to_string(),
        title_zh: manifest.title_zh.clone(),
        created_at: created_at.to_string(),
        entries: kept.len(),
        skipped_registry: skipped.len(),
    })
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    let mut items: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    items.sort();
    for p in items {
        if p.is_dir() {
            out.push(p.clone());
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_name_replaces_illegal_chars() {
        assert_eq!(safe_name("Cyberpunk 2077"), "Cyberpunk 2077");
        assert_eq!(safe_name("A/B:C*D?E"), "A_B_C_D_E");
        assert_eq!(safe_name("..."), "unnamed");
        assert_eq!(safe_name("  "), "unnamed");
    }

    #[test]
    fn bundles_backup_and_drops_registry_entries() {
        let tmp = std::env::temp_dir().join(format!("gm-bundle-test-{}", std::process::id()));
        let inst = tmp.join("DemoGame").join("2026-10-29_05-00-00");
        std::fs::create_dir_all(inst.join("存档")).unwrap();
        std::fs::create_dir_all(inst.join("注册表")).unwrap();
        std::fs::write(inst.join("存档").join("a.sav"), b"hello").unwrap();
        std::fs::write(inst.join("注册表").join("a.reg"), b"reg").unwrap();
        std::fs::write(
            inst.join("manifest.json"),
            serde_json::json!({
                "schema": 1,
                "title": "DemoGame",
                "createdAt": "2026-10-29_05-00-00",
                "entries": [
                    {"dir": "存档", "template": "x", "store": "folder", "contentType": "saves"},
                    {"dir": "注册表", "template": "y", "store": "reg", "contentType": "reg"}
                ]
            })
            .to_string(),
        )
        .unwrap();

        let out = tmp.join("bundle.zip");
        let info = build_bundle(&inst, "DemoGame", "2026-10-29_05-00-00", &out, true).unwrap();
        assert_eq!(info.entries, 1, "注册表条目应被排除");
        assert_eq!(info.skipped_registry, 1);
        assert_eq!(info.file_name, "DemoGame__2026-10-29_05-00-00.zip");
        assert!(info.size > 0);

        // 包内 manifest 也应只剩非注册表条目
        let f = File::open(&out).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let mut s = String::new();
        z.by_name("manifest.json").unwrap().read_to_string(&mut s).unwrap();
        assert!(s.contains("存档"));
        assert!(!s.contains("注册表"));

        std::fs::remove_dir_all(&tmp).ok();
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractInfo {
    pub game_dir: std::path::PathBuf,
    pub instance_dir: std::path::PathBuf,
    pub title: String,
    pub created_at: String,
    /// 解压出的文件数
    pub files: usize,
}

/// 从 zip 还原一份备份到 `<backups_root>/<游戏名>/<时间戳>/`。
///
/// 包内 `game.json`（归属索引）会被放到游戏目录下，其余文件进实例目录。
/// 目标目录已存在时追加 `_2`/`_3` 后缀，绝不覆盖本地已有备份。
pub fn extract_bundle(zip_path: &Path, backups_root: &Path) -> Result<ExtractInfo, String> {
    let file = File::open(zip_path).map_err(|e| format!("打开压缩包失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析压缩包失败: {e}"))?;

    // 先读 manifest 拿标题与时间戳
    let (title, created) = {
        let mut mf = archive
            .by_name("manifest.json")
            .map_err(|_| "压缩包里没有 manifest.json，可能不是本应用产出的备份".to_string())?;
        let mut text = String::new();
        mf.read_to_string(&mut text).map_err(|e| e.to_string())?;
        drop(mf);
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let title = v.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let created = v
            .get("createdAt")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        (title, created)
    };
    if title.trim().is_empty() {
        return Err("压缩包内 manifest 缺少 title".into());
    }
    let created = if created.trim().is_empty() {
        "restored".to_string()
    } else {
        created
    };

    let game_dir = backups_root.join(safe_name(&title));
    let instance_dir = unique_dir(&game_dir, &created);
    std::fs::create_dir_all(&instance_dir).map_err(|e| format!("创建目录失败: {e}"))?;

    let mut files = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(raw) = entry.enclosed_name() else {
            // 含 .. 或绝对路径的条目直接跳过，防目录穿越
            continue;
        };
        let name = raw.to_string_lossy().replace('\\', "/");

        // 归属索引写到游戏目录（与时间戳目录同级）
        if name == "game.json" {
            let dest = game_dir.join("game.json");
            write_stream(&mut entry, &dest)?;
            files += 1;
            continue;
        }

        let dest = instance_dir.join(&name);
        if name.ends_with('/') {
            std::fs::create_dir_all(&dest).ok();
            continue;
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        write_stream(&mut entry, &dest)?;
        files += 1;
    }

    Ok(ExtractInfo {
        game_dir,
        instance_dir,
        title,
        created_at: created,
        files,
    })
}

fn write_stream<R: Read>(reader: &mut R, dest: &Path) -> Result<(), String> {
    let mut out = File::create(dest).map_err(|e| format!("写入 {} 失败: {e}", dest.display()))?;
    std::io::copy(reader, &mut out).map_err(|e| e.to_string())?;
    Ok(())
}

/// 目标目录已存在时追加 _2/_3…，避免覆盖本地已有备份
fn unique_dir(game_dir: &Path, base: &str) -> std::path::PathBuf {
    let first = game_dir.join(safe_name(base));
    if !first.exists() {
        return first;
    }
    for n in 2..1000 {
        let cand = game_dir.join(format!("{}_{n}", safe_name(base)));
        if !cand.exists() {
            return cand;
        }
    }
    game_dir.join(format!("{}_{}", safe_name(base), std::process::id()))
}

#[cfg(test)]
mod extract_tests {
    use super::*;

    #[test]
    fn roundtrip_bundle_then_extract() {
        let tmp = std::env::temp_dir().join(format!("gm-extract-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let inst = tmp.join("src").join("DemoGame").join("2026-10-29_05-00-00");
        std::fs::create_dir_all(inst.join("存档")).unwrap();
        std::fs::write(inst.join("存档").join("a.sav"), b"save-bytes").unwrap();
        std::fs::write(
            inst.join("manifest.json"),
            serde_json::json!({
                "schema": 1, "title": "DemoGame", "createdAt": "2026-10-29_05-00-00",
                "entries": [{"dir": "存档", "template": "x", "store": "folder", "contentType": "saves"}]
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            tmp.join("src").join("DemoGame").join("game.json"),
            r#"{"schema":"gm.game-index/1","pcgwId":123,"title":"DemoGame"}"#,
        )
        .unwrap();

        let zip_path = tmp.join("b.zip");
        build_bundle(&inst, "DemoGame", "2026-10-29_05-00-00", &zip_path, true).unwrap();

        let restore_root = tmp.join("restore");
        std::fs::create_dir_all(&restore_root).unwrap();
        let info = extract_bundle(&zip_path, &restore_root).unwrap();

        assert_eq!(info.title, "DemoGame");
        assert!(info.instance_dir.join("存档").join("a.sav").exists());
        // 归属索引被还原到游戏目录
        let idx = info.game_dir.join("game.json");
        assert!(idx.exists());
        let text = std::fs::read_to_string(&idx).unwrap();
        assert!(text.contains("123"));

        // 再解压一次应换目录名，不覆盖
        let info2 = extract_bundle(&zip_path, &restore_root).unwrap();
        assert_ne!(info.instance_dir, info2.instance_dir);

        std::fs::remove_dir_all(&tmp).ok();
    }
}
