//! 通用 `gm.trainers/1` 目录来源。
//!
//! 风灵月影归档、小幸等来源只需要提供一份 catalog JSON，无需各自实现一堆
//! 解析代码。校验规则与 `fling.rs` 保持一致，额外的 `external` 包会标记为
//! “打开来源页面”，不会进入直接下载流程。

use super::{TrainerEntry, TrainerSource, TrainerSourceInfo, CATALOG_SCHEMA};

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

#[derive(Debug)]
pub struct CatalogSource {
    info: TrainerSourceInfo,
    generated_at: Option<String>,
    entries: Vec<TrainerEntry>,
}

impl CatalogSource {
    pub fn from_json(json: &str, expected_id: &str) -> Result<Self, String> {
        let file: CatalogFile =
            serde_json::from_str(json).map_err(|e| format!("catalog 解析失败: {e}"))?;
        if file.schema != CATALOG_SCHEMA {
            return Err(format!(
                "不支持的 catalog 格式 {}（需要 {CATALOG_SCHEMA}）",
                file.schema
            ));
        }
        if file.source.id != expected_id {
            return Err(format!(
                "source.id 应为 {expected_id}，实际是 {}",
                file.source.id
            ));
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
            if !entry.id.starts_with(&format!("{expected_id}/")) {
                return Err(format!("条目 id 应以 {expected_id}/ 开头：{}", entry.id));
            }
            if !seen.insert(entry.id.clone()) {
                return Err(format!("条目 id 重复：{}", entry.id));
            }
            for dl in &entry.downloads {
                if dl.url.trim().is_empty() {
                    return Err(format!("条目 {} 存在空下载地址的附件", entry.id));
                }
            }
        }

        Ok(Self {
            info: file.source,
            generated_at: file.generated_at,
            entries,
        })
    }

    pub fn generated_at(&self) -> Option<&str> {
        self.generated_at.as_deref()
    }
}

impl TrainerSource for CatalogSource {
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

    const CATALOG: &str = r#"{
        "schema": "gm.trainers/1",
        "source": { "id": "xiaoxing", "title": "小幸修改器", "homepage": "https://www.xiaoxingjie.com" },
        "trainers": [
            { "id": "xiaoxing/post-2591", "slug": "post-2591",
              "gameName": "Black Myth: Wukong", "zhName": "黑神话：悟空",
              "pageUrl": "https://www.xiaoxingjie.com/archives/65.html",
              "downloads": [
                { "file": "Black Myth Wukong Trainer", "url": "https://www.xiaoxingjie.com/archives/65.html",
                  "gameVersion": "1.7.0", "external": true }
              ] }
        ]
    }"#;

    #[test]
    fn loads_external_catalog_and_marks_the_package_external() {
        let source = CatalogSource::from_json(CATALOG, "xiaoxing").unwrap();
        assert_eq!(source.entries().len(), 1);
        let hit = super::super::TrainerHit::from_entry("xiaoxing", &source.entries()[0]);
        assert!(hit.download_url.is_none());
        assert_eq!(
            hit.external_url.as_deref(),
            Some("https://www.xiaoxingjie.com/archives/65.html")
        );
    }

    #[test]
    fn rejects_wrong_source_id() {
        let error = CatalogSource::from_json(CATALOG, "fling_archive").unwrap_err();
        assert!(error.contains("fling_archive"));
    }
}
