//! 风灵月影（FLiNG Trainer）来源：`gm.trainers/1` catalog 的加载与校验。
//!
//! catalog 由 scripts/trainers/ 管道自 flingtrainer.com 采集生成（打包在
//! `resources/trainers/` 下）。这里只做解析与不变式校验，不做网络请求。

use super::{CATALOG_SCHEMA, TrainerEntry, TrainerSource, TrainerSourceInfo};

/// 来源稳定 id（catalog 信封 source.id 必须等于该值）
pub const SOURCE_ID: &str = "fling";

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogFile {
    schema: String,
    #[serde(default)]
    generated_at: Option<String>,
    source: TrainerSourceInfo,
    #[serde(default)]
    trainers: Vec<TrainerEntry>,
}

/// 风灵月影来源。条目顺序即 catalog 内的顺序（游戏名排序）。
#[derive(Debug)]
pub struct FlingSource {
    info: TrainerSourceInfo,
    generated_at: Option<String>,
    entries: Vec<TrainerEntry>,
}

impl FlingSource {
    /// 从 catalog JSON 文本解析。校验：
    /// - `schema` 必须是当前支持的 [`CATALOG_SCHEMA`]
    /// - `source.id` 必须是 [`SOURCE_ID`]
    /// - 每个条目 id 必须以 `fling/` 开头且唯一；slug 非空
    pub fn from_json(json: &str) -> Result<Self, String> {
        let file: CatalogFile = serde_json::from_str(json).map_err(|e| format!("catalog 解析失败: {e}"))?;
        if file.schema != CATALOG_SCHEMA {
            return Err(format!(
                "不支持的 catalog 格式 {}（需要 {CATALOG_SCHEMA}）",
                file.schema
            ));
        }
        if file.source.id != SOURCE_ID {
            return Err(format!("source.id 应为 {SOURCE_ID}，实际是 {}", file.source.id));
        }

        let mut entries = file.trainers;
        let mut seen = std::collections::HashSet::new();
        for entry in &mut entries {
            if entry.game_name.trim().is_empty() {
                return Err(format!("条目 {} 缺少 gameName", entry.id));
            }
            if entry.slug.trim().is_empty() {
                return Err(format!("条目 {} 缺少 slug", entry.id));
            }
            if !entry.id.starts_with(&format!("{SOURCE_ID}/")) {
                return Err(format!("条目 id 应以 {SOURCE_ID}/ 开头：{}", entry.id));
            }
            if !seen.insert(entry.id.clone()) {
                return Err(format!("条目 id 重复：{}", entry.id));
            }
            // 附件地址必须可用（运行时下载还要求携带 pageUrl 作 Referer）
            for dl in &entry.downloads {
                if dl.url.trim().is_empty() {
                    return Err(format!("条目 {} 存在空下载地址的附件", entry.id));
                }
            }
        }

        Ok(FlingSource {
            info: file.source,
            generated_at: file.generated_at,
            entries,
        })
    }

    pub fn generated_at(&self) -> Option<&str> {
        self.generated_at.as_deref()
    }
}

impl TrainerSource for FlingSource {
    fn info(&self) -> &TrainerSourceInfo {
        &self.info
    }

    fn entries(&self) -> &[TrainerEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{
        "schema": "gm.trainers/1",
        "generatedAt": "2026-10-01T00:00:00Z",
        "source": { "id": "fling", "title": "FLiNG Trainer", "homepage": "https://flingtrainer.com", "listUrl": "https://flingtrainer.com/all-trainers/" },
        "stats": { "count": 1, "latestUpdate": "2025-10-17", "anomalies": { "withoutDownloads": 0 } },
        "trainers": [
            { "id": "fling/black-myth-wukong-trainer", "slug": "black-myth-wukong-trainer",
              "gameName": "Black Myth: Wukong", "zhName": null,
              "pageUrl": "https://flingtrainer.com/trainer/black-myth-wukong-trainer/",
              "updatedAt": "2025-10-17",
              "downloads": [
                { "file": "Black.Myth.Wukong.v1.0-v1.0.20.Plus.44.Trainer-FLiNG",
                  "url": "https://flingtrainer.com/downloads/aaa,,",
                  "dateAdded": "2025-10-17", "sizeBytes": 944128, "downloadCount": 1311424,
                  "gameVersion": "1.0-1.0.20", "optionsCount": 44 },
                { "file": "Black.Myth.Wukong.v1.0-v1.0.13.Plus.44.Trainer-FLiNG",
                  "url": "https://flingtrainer.com/downloads/bbb,,",
                  "dateAdded": "2024-12-14", "sizeBytes": 959488, "downloadCount": 2157079,
                  "gameVersion": "1.0-1.0.13", "optionsCount": 44 }
              ] }
        ]
    }"#;

    #[test]
    fn parses_minimal_catalog_with_envelope_intact() {
        let source = FlingSource::from_json(MINIMAL).unwrap();
        assert_eq!(source.info().id, "fling");
        assert_eq!(source.info().title, "FLiNG Trainer");
        assert_eq!(source.info().list_url.as_deref(), Some("https://flingtrainer.com/all-trainers/"));
        assert_eq!(source.generated_at(), Some("2026-10-01T00:00:00Z"));
        assert_eq!(source.entries().len(), 1);

        let entry = &source.entries()[0];
        assert_eq!(entry.game_name, "Black Myth: Wukong");
        assert_eq!(entry.updated_at.as_deref(), Some("2025-10-17"));
        assert_eq!(entry.downloads.len(), 2);
        // latest() 取首位（站点新→旧）
        let latest = entry.latest().unwrap();
        assert_eq!(latest.options_count, Some(44));
        assert_eq!(latest.game_version.as_deref(), Some("1.0-1.0.20"));
        assert_eq!(latest.size_bytes, Some(944128));
        // stats 字段是信封元数据，不进条目也不报错
    }

    #[test]
    fn tolerates_missing_optional_fields() {
        let json = r#"{
            "schema": "gm.trainers/1",
            "source": { "id": "fling", "title": "FLiNG", "homepage": "https://x" },
            "trainers": [
                { "id": "fling/doom-trainer", "slug": "doom-trainer",
                  "gameName": "DOOM", "pageUrl": "https://flingtrainer.com/trainer/doom-trainer/" }
            ]
        }"#;
        let source = FlingSource::from_json(json).unwrap();
        let entry = &source.entries()[0];
        assert!(entry.downloads.is_empty());
        assert_eq!(entry.zh_name, None);
        assert_eq!(entry.updated_at, None);
        assert!(entry.latest().is_none());
    }

    #[test]
    fn rejects_wrong_schema_and_wrong_source_id() {
        let bad_schema = MINIMAL.replace("gm.trainers/1", "gm.trainers/2");
        assert!(FlingSource::from_json(&bad_schema).unwrap_err().contains("不支持"));
        let bad_source = MINIMAL.replace("\"id\": \"fling\"", "\"id\": \"other\"");
        assert!(FlingSource::from_json(&bad_source).unwrap_err().contains("fling"));
        assert!(FlingSource::from_json("not json").unwrap_err().contains("解析失败"));
    }

    #[test]
    fn rejects_broken_invariants() {
        // id 前缀不对
        let bad_prefix = MINIMAL.replace("fling/black-myth-wukong-trainer", "other/black-myth-wukong-trainer");
        assert!(FlingSource::from_json(&bad_prefix).unwrap_err().contains("开头"));
        // id 重复：注入同 id 条目
        let dupe = MINIMAL.replace(
            r#""trainers": ["#,
            r#""trainers": [ {"id": "fling/black-myth-wukong-trainer", "slug": "x", "gameName": "X", "pageUrl": "https://x"}, "#,
        );
        assert!(FlingSource::from_json(&dupe).unwrap_err().contains("重复"));
        // gameName 缺失
        let empty_name = MINIMAL.replace("Black Myth: Wukong", " ");
        assert!(FlingSource::from_json(&empty_name).unwrap_err().contains("gameName"));
    }
}
