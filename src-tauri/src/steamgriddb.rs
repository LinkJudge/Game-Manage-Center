//! SteamGridDB 第二阶段集成：按游戏名搜索、列出候选图标、手动应用、批量补全。
//!
//! API Key 只保存在本机 settings.json，仅作为 Bearer 头发给 SteamGridDB。
//! 候选图 URL 必须来自 SteamGridDB 官方 CDN，避免前端传入任意 URL 造成 SSRF。

use crate::game_cover;
use image::{imageops::FilterType, DynamicImage, GenericImageView, ImageFormat};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

const API_BASE: &str = "https://www.steamgriddb.com/api/v2";
const ICON_SIZE: u32 = 256;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SgdbCandidate {
    pub id: u64,
    pub url: String,
    pub thumb: String,
    pub width: u32,
    pub height: u32,
    pub style: Option<String>,
    pub mime: String,
    pub author: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SgdbOptions {
    pub game_id: u64,
    pub game_name: String,
    pub candidates: Vec<SgdbCandidate>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverLookup {
    pub title: String,
    pub pcgw_id: Option<i64>,
    pub steam_id: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FillSummary {
    pub total: usize,
    pub filled: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("GameManageCenter/0.4")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

fn get_json(
    client: &reqwest::blocking::Client,
    api_key: &str,
    url: &str,
    query: &[(&str, &str)],
) -> Result<Value, String> {
    let response = client
        .get(url)
        .query(query)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .map_err(|e| format!("连接 SteamGridDB 失败: {e}"))?;
    let status = response.status();
    let body = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(if status.as_u16() == 401 || status.as_u16() == 403 {
            "SteamGridDB API Key 无效或无权限".to_string()
        } else {
            format!("SteamGridDB 返回 {status}: {}", body.chars().take(200).collect::<String>())
        });
    }
    serde_json::from_str(&body).map_err(|e| format!("解析 SteamGridDB 响应失败: {e}"))
}

fn normalize(value: &str) -> String {
    value
        .normalize_unicode()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(c))
        .collect()
}

trait NormalizeUnicode {
    fn normalize_unicode(&self) -> String;
}

impl NormalizeUnicode for str {
    fn normalize_unicode(&self) -> String {
        // 只处理常见全角字符，避免为标题匹配引入额外 Unicode 依赖
        self.chars()
            .map(|c| match c {
                '\u{ff01}'..='\u{ff5e}' => {
                    char::from_u32(c as u32 - 0xfee0).unwrap_or(c)
                }
                '\u{3000}' => ' ',
                _ => c,
            })
            .collect()
    }
}

fn find_game_id(
    client: &reqwest::blocking::Client,
    api_key: &str,
    title: &str,
    steam_appid: Option<&str>,
) -> Result<(u64, String), String> {
    if let Some(appid) = steam_appid {
        let url = format!("{API_BASE}/games/steam/{appid}");
        if let Ok(value) = get_json(client, api_key, &url, &[]) {
            if let Some(data) = value.get("data") {
                if let Some(id) = data.get("id").and_then(Value::as_u64) {
                    let name = data
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(title)
                        .to_string();
                    return Ok((id, name));
                }
            }
        }
    }

    let encoded = urlencoding::encode(title);
    let url = format!("{API_BASE}/search/autocomplete/{encoded}");
    let value = get_json(client, api_key, &url, &[])?;
    let list = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("SteamGridDB 没有找到「{title}」"))?;
    if list.is_empty() {
        return Err(format!("SteamGridDB 没有找到「{title}」"));
    }
    let wanted = normalize(title);
    let mut chosen = &list[0];
    if let Some(exact) = list.iter().find(|entry| {
        entry
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| normalize(name) == wanted)
    }) {
        chosen = exact;
    }
    let id = chosen
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| "SteamGridDB 游戏条目缺少 id".to_string())?;
    let name = chosen
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(title)
        .to_string();
    Ok((id, name))
}

fn parse_candidates(value: &Value) -> Vec<SgdbCandidate> {
    let Some(list) = value.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            let url = entry.get("url").and_then(Value::as_str)?.to_string();
            let thumb = entry
                .get("thumb")
                .and_then(Value::as_str)
                .unwrap_or(&url)
                .to_string();
            Some(SgdbCandidate {
                id: entry.get("id").and_then(Value::as_u64)?,
                url,
                thumb,
                width: entry.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                height: entry.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
                style: entry
                    .get("style")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                mime: entry
                    .get("mime")
                    .and_then(Value::as_str)
                    .unwrap_or("image/png")
                    .to_string(),
                author: entry
                    .get("author")
                    .and_then(|a| a.get("name"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

pub fn search_candidates(
    api_key: &str,
    title: &str,
    steam_appid: Option<&str>,
) -> Result<SgdbOptions, String> {
    if api_key.trim().is_empty() {
        return Err("请先在 设置 → 游戏图标 中配置 SteamGridDB API Key".to_string());
    }
    let client = client()?;
    let (game_id, game_name) = find_game_id(&client, api_key, title, steam_appid)?;
    let icons_url = format!("{API_BASE}/icons/game/{game_id}");
    let filters = [
        ("types", "static"),
        ("nsfw", "false"),
        ("humor", "false"),
    ];
    let mut candidates = get_json(&client, api_key, &icons_url, &filters)
        .map(|value| parse_candidates(&value))
        .unwrap_or_default();

    // 有些游戏没有方形 icon，退回 600x900 封面网格
    if candidates.is_empty() {
        let grids_url = format!("{API_BASE}/grids/game/{game_id}");
        let grids = get_json(&client, api_key, &grids_url, &filters)
            .map(|value| parse_candidates(&value))
            .unwrap_or_default();
        candidates = grids
            .into_iter()
            .filter(|candidate| candidate.width == candidate.height || candidate.height > 300)
            .take(60)
            .collect();
    }

    if candidates.is_empty() {
        return Err(format!("SteamGridDB 中「{game_name}」还没有可用图标"));
    }
    candidates.truncate(60);
    Ok(SgdbOptions {
        game_id,
        game_name,
        candidates,
    })
}

fn trusted_image_url(url: &str) -> bool {
    url.starts_with("https://cdn2.steamgriddb.com/")
        || url.starts_with("https://cdn.steamgriddb.com/")
        || url.starts_with("https://steamgriddb.com/")
}

fn normalize_icon(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let side = width.min(height);
    let cropped = image.crop_imm((width - side) / 2, (height - side) / 2, side, side);
    let resized = cropped
        .resize_exact(ICON_SIZE, ICON_SIZE, FilterType::Lanczos3)
        .to_rgba8();
    let mut out = Vec::new();
    {
        let mut cursor = Cursor::new(&mut out);
        DynamicImage::ImageRgba8(resized)
            .write_to(&mut cursor, ImageFormat::Png)
            .ok()?;
    }
    Some(out)
}

fn download_icon(url: &str) -> Result<Vec<u8>, String> {
    if !trusted_image_url(url) {
        return Err("候选图标地址不受信任".to_string());
    }
    let response = client()?
        .get(url)
        .send()
        .map_err(|e| format!("下载候选图标失败: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("下载候选图标失败: HTTP {}", response.status()));
    }
    let bytes = response.bytes().map_err(|e| e.to_string())?;
    Ok(bytes.to_vec())
}

/// 应用 SteamGridDB 候选图，返回新的 data URL
#[allow(clippy::too_many_arguments)]
pub fn apply_candidate(
    app_data_dir: &Path,
    db_path: &Path,
    title: &str,
    pcgw_id: Option<i64>,
    steam_id: Option<String>,
    candidate_url: &str,
) -> Result<String, String> {
    let bytes = download_icon(candidate_url)?;
    let png = normalize_icon(&bytes).ok_or_else(|| "候选图无法解码".to_string())?;
    let steam_appid =
        game_cover::resolve_steam_appid(db_path, title, pcgw_id, steam_id.as_deref());
    let key = game_cover::cache_key(title, pcgw_id, steam_appid.as_deref());
    game_cover::replace_cached_image(app_data_dir, &key, &png, true)
        .ok_or_else(|| "写入封面缓存失败".to_string())
}

/// 批量补全缺失封面（已缓存会自动跳过，最多处理 40 个）
pub fn fill_missing(
    api_key: &str,
    app_data_dir: &Path,
    db_path: &Path,
    requests: Vec<CoverLookup>,
) -> Result<FillSummary, String> {
    if api_key.trim().is_empty() {
        return Err("请先在 设置 → 游戏图标 中配置 SteamGridDB API Key".to_string());
    }
    let requests: Vec<_> = requests.into_iter().take(40).collect();
    let mut summary = FillSummary {
        total: requests.len(),
        ..Default::default()
    };
    for request in requests {
        let appid = game_cover::resolve_steam_appid(
            db_path,
            &request.title,
            request.pcgw_id,
            request.steam_id.as_deref(),
        );
        let key = game_cover::cache_key(&request.title, request.pcgw_id, appid.as_deref());
        if game_cover::cached_cover_exists(app_data_dir, &key) {
            summary.skipped += 1;
            continue;
        }
        match search_candidates(api_key, &request.title, appid.as_deref())
            .and_then(|options| {
                let candidate = options.candidates.first().cloned().ok_or_else(|| {
                    format!("{}：没有可用图标", request.title)
                })?;
                apply_candidate(
                    app_data_dir,
                    db_path,
                    &request.title,
                    request.pcgw_id,
                    request.steam_id.clone(),
                    &candidate.url,
                )
                .map(|_| ())
            }) {
            Ok(()) => summary.filled += 1,
            Err(e) => {
                summary.failed += 1;
                if summary.errors.len() < 8 {
                    summary.errors.push(e);
                }
            }
        }
        // 温和限速：批量补全不追求极限速度，避免触发 SteamGridDB 频控
        std::thread::sleep(Duration::from_millis(600));
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_titles_for_exact_match() {
        assert_eq!(normalize("Half-Life 2"), "halflife2");
        assert_eq!(normalize("《黑神话：悟空》"), "黑神话悟空");
    }

    #[test]
    fn rejects_external_image_hosts() {
        assert!(trusted_image_url(
            "https://cdn2.steamgriddb.com/file/sgdb-cdn/icon/abc.png"
        ));
        assert!(!trusted_image_url("https://example.com/evil.png"));
        assert!(!trusted_image_url("http://cdn2.steamgriddb.com/icon.png"));
    }
}
