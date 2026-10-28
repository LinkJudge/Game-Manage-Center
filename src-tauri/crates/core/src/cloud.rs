//! 云备份打包：把一个备份实例打成一个 zip，用于上传到 WebDAV。
//!
//! 设计要点：
//! - **只打包，不压缩**（`CompressionMethod::Stored`）。存档文件本身多为已压缩格式
//!   （.rpgsave / .sav / .dat），再 deflate 收益极小却明显更慢。
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
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    zip.start_file("manifest.json", opts).map_err(|e| e.to_string())?;
    zip.write_all(manifest_json.as_bytes()).map_err(|e| e.to_string())?;

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
            zip.start_file(name, opts).map_err(|e| e.to_string())?;
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
                    zip.add_directory(format!("{rel}/"), opts).map_err(|e| e.to_string())?;
                    continue;
                }
                let mut f = File::open(&item).map_err(|e| e.to_string())?;
                zip.start_file(rel, opts).map_err(|e| e.to_string())?;
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
