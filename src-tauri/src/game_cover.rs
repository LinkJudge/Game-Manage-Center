//! 游戏封面抓取与缓存。
//!
//! 顺序：本地缓存 → Steam 本机 librarycache → Steam 官方 CDN → EXE 内嵌图标。
//! 最终统一裁成 256×256 方图缓存到应用数据目录，前端以 data URL 使用，
//! 避免把第三方美术资源打进安装包。

use base64::Engine;
use gm_core::database::Database;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat};
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const CACHE_DIR: &str = "GameArt";
const COVER_SIZE: u32 = 256;

pub(crate) fn cache_key(
    title: &str,
    pcgw_id: Option<i64>,
    steam_appid: Option<&str>,
) -> String {
    if let Some(appid) = steam_appid {
        return format!("steam-{appid}");
    }
    if let Some(id) = pcgw_id {
        return format!("game-{id}");
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    title.trim().to_lowercase().hash(&mut hasher);
    format!("title-{:016x}", hasher.finish())
}

/// 封面缓存的候选文件名与 MIME：**手动选图排在自动封面之前**。
///
/// 手动选图写 `{key}.custom.png|jpg`，与自动链路使用的 `{key}.jpg|png` 完全分离，
/// 自动抓取和"清理缓存"都不会动它 —— 否则用户挑过的图标会在下次启动时被
/// Steam CDN 的自动封面顶掉（2026-10-28 戢翎遇到的正是这个问题）。
const CACHE_CANDIDATES: [(&str, &str); 4] = [
    ("custom.png", "image/png"),
    ("custom.jpg", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("png", "image/png"),
];

fn cached_data_url(dir: &Path, key: &str) -> Option<String> {
    for (ext, mime) in CACHE_CANDIDATES {
        let path = dir.join(CACHE_DIR).join(format!("{key}.{ext}"));
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if bytes.is_empty() {
            let _ = std::fs::remove_file(path);
            continue;
        }
        return Some(format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ));
    }
    None
}

pub(crate) fn cached_cover_exists(dir: &Path, key: &str) -> bool {
    CACHE_CANDIDATES.iter().any(|(ext, _)| {
        dir.join(CACHE_DIR)
            .join(format!("{key}.{ext}"))
            .is_file()
    })
}

fn write_cache(dir: &Path, key: &str, bytes: &[u8], is_png: bool) -> Option<PathBuf> {
    let cache_dir = dir.join(CACHE_DIR);
    std::fs::create_dir_all(&cache_dir).ok()?;
    let (ext, other) = if is_png { ("png", "jpg") } else { ("jpg", "png") };
    let path = cache_dir.join(format!("{key}.{ext}"));
    std::fs::write(&path, bytes).ok()?;
    // 同一张图只留一份：否则查找顺序会读到另一种扩展名下的过期图片
    let _ = std::fs::remove_file(cache_dir.join(format!("{key}.{other}")));
    Some(path)
}

fn normalize_cover(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || is_placeholder(&image) {
        return None;
    }
    let side = width.min(height);
    let crop = image.crop_imm((width - side) / 2, (height - side) / 2, side, side);
    let resized = crop.resize_exact(COVER_SIZE, COVER_SIZE, FilterType::Lanczos3);
    let mut out = Vec::new();
    {
        let mut cursor = Cursor::new(&mut out);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, 88)
            .encode_image(&DynamicImage::ImageRgb8(resized.to_rgb8()))
            .ok()?;
    }
    Some(out)
}

/// Steam 偶尔会给未上线/地区不可见的条目返回纯灰占位图。
/// 采样亮度范围和量化色数，过于平坦的图直接跳过。
fn is_placeholder(image: &DynamicImage) -> bool {
    let thumb = image.thumbnail(32, 32).to_rgb8();
    let mut min_luma = 255u32;
    let mut max_luma = 0u32;
    let mut colors = std::collections::HashSet::new();
    for pixel in thumb.pixels() {
        let [r, g, b] = pixel.0;
        let luma = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
        min_luma = min_luma.min(luma);
        max_luma = max_luma.max(luma);
        colors.insert((r / 16, g / 16, b / 16));
    }
    max_luma.saturating_sub(min_luma) < 18 && colors.len() < 6
}

fn extract_steam_appid(raw: &str) -> Option<String> {
    let digits: String = raw
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    (!digits.is_empty()).then_some(digits)
}

/// 通过 wiki id / 标题从本地数据库补全 Steam AppID（备份记录和修改器只有标题时也适用）
fn lookup_steam_appid(db_path: &Path, title: &str, pcgw_id: Option<i64>) -> Option<String> {
    let db = Database::open_readonly(db_path).ok()?;
    let entry = if let Some(id) = pcgw_id {
        db.get_game(id).ok().flatten()
    } else {
        db.list_games_by_titles(&[title]).ok()?.into_iter().next()
    }?;
    entry.steam_id.as_deref().and_then(extract_steam_appid)
}

pub(crate) fn resolve_steam_appid(
    db_path: &Path,
    title: &str,
    pcgw_id: Option<i64>,
    provided: Option<&str>,
) -> Option<String> {
    provided
        .and_then(extract_steam_appid)
        .or_else(|| lookup_steam_appid(db_path, title, pcgw_id))
}

/// 手动选图：写入独立的 custom 槽位，自动封面与清理缓存都不会覆盖它
pub(crate) fn replace_cached_image(
    app_data_dir: &Path,
    key: &str,
    bytes: &[u8],
    is_png: bool,
) -> Option<String> {
    let cache_dir = app_data_dir.join(CACHE_DIR);
    std::fs::create_dir_all(&cache_dir).ok()?;
    let (ext, other) = if is_png {
        ("custom.png", "custom.jpg")
    } else {
        ("custom.jpg", "custom.png")
    };
    std::fs::write(cache_dir.join(format!("{key}.{ext}")), bytes).ok()?;
    let _ = std::fs::remove_file(cache_dir.join(format!("{key}.{other}")));
    // 自动封面已被手动选择取代，删掉避免占空间（不影响显示：custom 优先）
    for plain in ["jpg", "png"] {
        let _ = std::fs::remove_file(cache_dir.join(format!("{key}.{plain}")));
    }
    let mime = if is_png { "image/png" } else { "image/jpeg" };
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

fn steam_cache_dirs() -> &'static Vec<PathBuf> {
    static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    DIRS.get_or_init(|| {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut push = |root: PathBuf| {
            let cache = root.join("appcache").join("librarycache");
            if cache.is_dir() && !dirs.iter().any(|p| p == &cache) {
                dirs.push(cache);
            }
        };

        for key in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Ok(base) = std::env::var(key) {
                push(PathBuf::from(base).join("Steam"));
            }
        }

        #[cfg(windows)]
        {
            if let Ok(env) = std::panic::catch_unwind(gm_core::detect::detect) {
                if let Some(path) = env.steam_path {
                    push(PathBuf::from(path));
                }
                for root in env
                    .install_paths
                    .iter()
                    .filter(|r| r.platform.eq_ignore_ascii_case("Steam"))
                {
                    // detect 的 Steam 根是 <library>/steamapps/common
                    if let Some(library_root) = Path::new(&root.path)
                        .parent()
                        .and_then(Path::parent)
                    {
                        push(library_root.to_path_buf());
                    }
                }
            }
        }
        dirs
    })
}

fn find_local_steam_cover(appid: &str, install_path: Option<&str>) -> Option<Vec<u8>> {
    let mut dirs = steam_cache_dirs().to_vec();
    if let Some(install) = install_path {
        let lower = install.to_lowercase();
        if let Some(pos) = lower.find("\\steamapps") {
            let root = PathBuf::from(&install[..pos]);
            let cache = root.join("appcache").join("librarycache");
            if cache.is_dir() && !dirs.iter().any(|p| p == &cache) {
                dirs.push(cache);
            }
        }
    }

    for dir in dirs {
        for name in [
            format!("{appid}_library_600x900_2x.jpg"),
            format!("{appid}_library_600x900.jpg"),
            format!("{appid}_header.jpg"),
        ] {
            let path = dir.join(&name);
            if let Ok(bytes) = std::fs::read(&path) {
                if !bytes.is_empty() {
                    return Some(bytes);
                }
            }
        }
    }
    None
}

fn http_client() -> Option<&'static reqwest::blocking::Client> {
    static CLIENT: OnceLock<Option<reqwest::blocking::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .user_agent("GameManageCenter/0.4")
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .ok()
        })
        .as_ref()
}

fn steam_cover_urls(appid: &str) -> [String; 3] {
    [
        format!(
            "https://shared.steamstatic.com/store_item_assets/steam/apps/{appid}/library_600x900_2x.jpg"
        ),
        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/library_600x900.jpg"),
        format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{appid}/header.jpg"),
    ]
}

fn download_bytes(url: &str) -> Option<Vec<u8>> {
    let client = http_client()?;
    let response = client.get(url).send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().ok()?;
    (!bytes.is_empty()).then_some(bytes.to_vec())
}

fn find_game_executable(root: &Path) -> Option<PathBuf> {
    fn collect(dir: &Path, depth: u8, out: &mut Vec<(PathBuf, u64)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten().take(300) {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                if depth > 0 {
                    collect(&path, depth - 1, out);
                }
                continue;
            }
            if !path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_lowercase();
            if [
                "unins", "setup", "redist", "vcredist", "directx", "dotnet", "crashreport",
                "launcher_installer",
            ]
            .iter()
            .any(|part| name.contains(part))
            {
                continue;
            }
            if meta.len() >= 64 * 1024 {
                out.push((path, meta.len()));
            }
        }
    }

    let mut candidates = Vec::new();
    collect(root, 1, &mut candidates);
    candidates
        .into_iter()
        .max_by_key(|(_, size)| *size)
        .map(|(path, _)| path)
}

#[cfg(windows)]
fn extract_exe_icon_png(exe: &Path) -> Option<Vec<u8>> {
    let icon = windows_icons::get_icon_by_path_with_size(
        exe.to_string_lossy().as_ref(),
        windows_icons::IconSize::ExtraLarge,
    )
    .ok()?;
    let image = DynamicImage::ImageRgba8(icon).resize_exact(
        COVER_SIZE,
        COVER_SIZE,
        FilterType::Lanczos3,
    );
    let mut out = Vec::new();
    {
        let mut cursor = Cursor::new(&mut out);
        image.write_to(&mut cursor, ImageFormat::Png).ok()?;
    }
    Some(out)
}

#[cfg(not(windows))]
fn extract_exe_icon_png(_exe: &Path) -> Option<Vec<u8>> {
    None
}

/// 获取（并缓存）一个游戏的方形封面。找不到时返回 None，前端回退字母图标。
pub fn fetch_cover(
    app_data_dir: &Path,
    db_path: &Path,
    title: &str,
    pcgw_id: Option<i64>,
    steam_id: Option<String>,
    install_path: Option<String>,
) -> Result<Option<String>, String> {
    let steam_appid = resolve_steam_appid(
        db_path,
        title,
        pcgw_id,
        steam_id.as_deref(),
    );
    let key = cache_key(title, pcgw_id, steam_appid.as_deref());

    if let Some(url) = cached_data_url(app_data_dir, &key) {
        return Ok(Some(url));
    }

    // 1. Steam 本机缓存（离线、零请求）
    if let Some(appid) = steam_appid.as_deref() {
        if let Some(bytes) = find_local_steam_cover(appid, install_path.as_deref()) {
            if let Some(normalized) = normalize_cover(&bytes) {
                if write_cache(app_data_dir, &key, &normalized, false).is_some() {
                    return Ok(Some(format!(
                        "data:image/jpeg;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(normalized)
                    )));
                }
            }
        }

        // 2. Steam 官方 CDN（逐个候选，跳过纯色占位图）
        for url in steam_cover_urls(appid) {
            let Some(bytes) = download_bytes(&url) else {
                continue;
            };
            let Some(normalized) = normalize_cover(&bytes) else {
                continue;
            };
            if write_cache(app_data_dir, &key, &normalized, false).is_some() {
                return Ok(Some(format!(
                    "data:image/jpeg;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(normalized)
                )));
            }
        }
    }

    // 3. 非 Steam 或没有封面：尝试安装目录主程序图标
    if let Some(install) = install_path.as_deref() {
        let root = Path::new(install);
        if root.is_dir() {
            if let Some(exe) = find_game_executable(root) {
                if let Some(png) = extract_exe_icon_png(&exe) {
                    if write_cache(app_data_dir, &key, &png, true).is_some() {
                        return Ok(Some(format!(
                            "data:image/png;base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(png)
                        )));
                    }
                }
            }
        }
    }

    Ok(None)
}

/// 清空自动封面缓存（返回删除数量）。**手动选择的图标（*.custom.*）保留**，
/// 用户挑过的封面不应该被一次"清理缓存"抹掉。
pub fn clear_cache(app_data_dir: &Path) -> Result<u64, String> {
    let dir = app_data_dir.join(CACHE_DIR);
    if !dir.is_dir() {
        return Ok(0);
    }
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.contains(".custom.") {
            continue;
        }
        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| {
                e.eq_ignore_ascii_case("jpg")
                    || e.eq_ignore_ascii_case("jpeg")
                    || e.eq_ignore_ascii_case("png")
            });
        if is_image && std::fs::remove_file(&path).is_ok() {
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gm-cover-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(CACHE_DIR)).unwrap();
        dir
    }

    /// 回归：手动选图必须独立存放，自动抓封面与"清理缓存"都不能动它
    #[test]
    fn custom_cover_survives_cache_clear_and_auto_overwrite() {
        let dir = temp_dir("custom");
        let key = "steam-413150";

        // 先有一张自动封面
        write_cache(&dir, key, b"auto-cover", false).unwrap();
        assert!(dir.join(CACHE_DIR).join(format!("{key}.jpg")).is_file());

        // 用户手动选图 → 写入 custom 槽，自动图被清掉
        replace_cached_image(&dir, key, b"hand-picked", true).unwrap();
        assert!(dir.join(CACHE_DIR).join(format!("{key}.custom.png")).is_file());
        assert!(!dir.join(CACHE_DIR).join(format!("{key}.jpg")).is_file());

        // 自动链路重新抓到封面：custom 仍然优先
        write_cache(&dir, key, b"auto-again", false).unwrap();
        let url = cached_data_url(&dir, key).unwrap();
        assert!(
            url.contains(&base64::engine::general_purpose::STANDARD.encode(b"hand-picked")),
            "custom 封面应优先于自动封面"
        );

        // 清理缓存：只删自动封面，custom 保留
        let removed = clear_cache(&dir).unwrap();
        assert_eq!(removed, 1);
        assert!(dir.join(CACHE_DIR).join(format!("{key}.custom.png")).is_file());
        assert!(cached_data_url(&dir, key).is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 回归：同键只留一种扩展名，避免查找顺序读到过期图片
    #[test]
    fn write_cache_drops_other_extension() {
        let dir = temp_dir("ext");
        let key = "game-42";
        std::fs::write(dir.join(CACHE_DIR).join(format!("{key}.png")), b"old-png").unwrap();
        write_cache(&dir, key, b"new-jpg", false).unwrap();
        assert!(!dir.join(CACHE_DIR).join(format!("{key}.png")).is_file());
        let url = cached_data_url(&dir, key).unwrap();
        assert!(url.contains(&base64::engine::general_purpose::STANDARD.encode(b"new-jpg")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn steam_appid_parses_first_number() {
        assert_eq!(extract_steam_appid("239140").as_deref(), Some("239140"));
        assert_eq!(
            extract_steam_appid("239140 (Dying Light)").as_deref(),
            Some("239140")
        );
        assert_eq!(extract_steam_appid("n/a"), None);
    }

    #[test]
    fn cache_key_prefers_steam_id() {
        assert_eq!(cache_key("Dying Light", Some(1), Some("239140")), "steam-239140");
        assert_eq!(cache_key("Dying Light", Some(1), None), "game-1");
        assert!(cache_key("Dying Light", None, None).starts_with("title-"));
    }

    #[test]
    fn flat_images_are_placeholders() {
        let flat =
            DynamicImage::ImageRgb8(RgbImage::from_pixel(16, 16, Rgb([80, 80, 80])));
        assert!(is_placeholder(&flat));

        let mut varied = RgbImage::from_pixel(16, 16, Rgb([20, 30, 40]));
        for (x, _, pixel) in varied.enumerate_pixels_mut() {
            if x > 8 {
                *pixel = Rgb([230, 210, 190]);
            }
        }
        assert!(!is_placeholder(&DynamicImage::ImageRgb8(varied)));
    }
}
