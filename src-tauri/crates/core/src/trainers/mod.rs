//! 修改器模块 —— 目录（catalog）的加载、检索与游戏匹配。
//!
//! 愿景是聚合多个修改器来源（风灵月影、后续其他站点）。每个来源一份自描述的
//! catalog JSON 文件（格式 `gm.trainers/1`，见 scripts/trainers/README.md），
//! 各来源实现 [`TrainerSource`] 并向 [`TrainerRegistry`] 登记；检索与游戏匹配
//! 只面向注册表编程，不关心具体来源。
//!
//! 行为规格见 SPEC.md「修改器目录」章节：归一化、检索语义、匹配语义。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

pub mod catalog;
pub mod fling;
pub mod fuzzy;
pub mod install;

/// 内置的通用来源：风灵月影历史归档（直接下载）
pub const FLING_ARCHIVE_SOURCE_ID: &str = "fling_archive";
/// 内置的通用来源：小幸修改器（官网/网盘页，仅索引）
pub const XIAOXING_SOURCE_ID: &str = "xiaoxing";

/// 当前支持的 catalog 格式版本（加载时不匹配即拒绝）
pub const CATALOG_SCHEMA: &str = "gm.trainers/1";

// ======================================================================
// 数据类型（与 catalog JSON 的字段一一对应，camelCase 通行于前端契约）
// ======================================================================

/// 来源元信息（catalog 信封里的 source 块）
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerSourceInfo {
    pub id: String,
    pub title: String,
    pub homepage: String,
    #[serde(default)]
    pub list_url: Option<String>,
}

/// 一个可下载的修改器包（同一游戏支持不同游戏版本的包有多个，按新→旧排列）
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerDownload {
    pub file: String,
    pub url: String,
    #[serde(default)]
    pub date_added: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub download_count: Option<u64>,
    /// 该包支持的游戏版本区间（不是修改器自身版本）
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub options_count: Option<u32>,
    /// 非直接下载：URL 指向详情页；前端应打开浏览器而不是直接安装
    #[serde(default)]
    pub external: bool,
}

/// 一个修改器条目（对应来源站点的一个详情页）
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerEntry {
    /// 全局稳定 id：`<source>/<slug>`
    pub id: String,
    pub slug: String,
    /// 来源站点的英文名（已剥 " Trainer" 后缀）
    pub game_name: String,
    /// 预留：catalog 可携带中文名；通常为空，由引擎用游戏库 zh_CN 回填
    #[serde(default)]
    pub zh_name: Option<String>,
    /// 版本变体别名（如 "Monster Hunter World: Iceborne" ↔ "Monster Hunter: World"），
    /// 由管道的手工别名表登记；检索与游戏匹配时与 game_name 同权
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 详情页地址（下载时作为 Referer，见 scripts/trainers/README.md）
    pub page_url: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub downloads: Vec<TrainerDownload>,
}

impl TrainerEntry {
    /// 最新包（downloads 首位）；无附件时 None
    pub fn latest(&self) -> Option<&TrainerDownload> {
        self.downloads.first()
    }
}

// ======================================================================
// 检索结果与游戏匹配
// ======================================================================

/// 检索/匹配结果的条目摘要（前端展示形态）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerHit {
    pub source_id: String,
    pub id: String,
    pub game_name: String,
    pub zh_name: Option<String>,
    /// 游戏库里的官方繁体名（繁体界面用；没有则为空，前端回退原名）
    pub zh_hant: Option<String>,
    /// 匹配到的游戏库官方名（英文原名）；来源名不是目标语言时用它兜底
    pub official_name: Option<String>,
    pub page_url: String,
    pub updated_at: Option<String>,
    /// 最新包的选项数（如 Plus.44 → 44）
    pub options_count: Option<u32>,
    /// 最新包支持的版本区间
    pub game_version: Option<String>,
    /// 最新包的下载地址（下载命令的入参）
    pub download_url: Option<String>,
    /// 非直接下载来源的详情页/网盘页地址
    pub external_url: Option<String>,
    pub size_bytes: Option<u64>,
    pub download_count: Option<u64>,
    /// 附件总数（该游戏可用的包数量）
    pub total_downloads: usize,
    /// true = 只靠「重制/增强」类后缀兜底匹配上的，版本可能不同，建议界面提示
    #[serde(default)]
    pub edition_fallback: bool,
}

impl TrainerHit {
    pub fn from_entry(source_id: &str, entry: &TrainerEntry) -> Self {
        let latest = entry.latest();
        TrainerHit {
            source_id: source_id.to_string(),
            id: entry.id.clone(),
            game_name: entry.game_name.clone(),
            zh_name: entry.zh_name.clone(),
            zh_hant: None,
            official_name: None,
            page_url: entry.page_url.clone(),
            updated_at: entry.updated_at.clone(),
            options_count: latest.and_then(|d| d.options_count),
            game_version: latest.and_then(|d| d.game_version.clone()),
            download_url: latest.filter(|d| !d.external).map(|d| d.url.clone()),
            external_url: latest
                .filter(|d| d.external)
                .map(|_| entry.page_url.clone()),
            size_bytes: latest.and_then(|d| d.size_bytes),
            download_count: latest.and_then(|d| d.download_count),
            total_downloads: entry.downloads.len(),
            edition_fallback: false,
        }
    }

    /// zhName 缺失时用游戏库回填（已有值不覆盖）
    pub fn with_zh_fallback(mut self, zh: Option<String>) -> Self {
        if self.zh_name.is_none() {
            self.zh_name = zh;
        }
        self
    }

    /// 繁体名回填（只来自游戏库，不覆盖已有值）
    pub fn with_zh_hant_fallback(mut self, hant: Option<String>) -> Self {
        if self.zh_hant.is_none() {
            self.zh_hant = hant;
        }
        self
    }

    /// 记下匹配到的游戏库官方名（英文原名），供"来源名不是目标语言"时兜底
    pub fn with_official_name(mut self, name: Option<String>) -> Self {
        if self.official_name.is_none() {
            self.official_name = name;
        }
        self
    }
}

/// 一款已安装游戏的匹配结果（顺序与输入游戏列表一致）
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameTrainerMatch {
    pub title: String,
    pub zh_name: Option<String>,
    pub matches: Vec<TrainerHit>,
}

/// 一个已安装包的可更新信息：目录里该条目的最新包与本地安装的不是同一个
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerUpdateInfo {
    pub trainer_id: String,
    pub game_name: String,
    pub zh_name: Option<String>,
    /// 旧包目录（更新成功后由调用方删除）
    pub old_folder: String,
    pub new_download: TrainerDownload,
}

// ======================================================================
// 名称归一化（检索与匹配共用；规则见 SPEC.md）
// ======================================================================

/// 名称归一化：保留 ASCII 字母数字与 CJK 表意文字，其余（空白、标点、全角、
/// 符号）全部丢弃并转小写；ASCII 数字段转为罗马数字小写（"3"→"iii"），
/// 使 "DOOM 3" 与 "DOOM III"、"gta5" 与 "GTA V" 等价。检索与匹配共用。
/// 同一游戏若官网（fling）有条目，就隐藏旧档（fling_archive）那条。
///
/// 依据：官网对同一游戏提供更新、更全的版本（Anno 1800 官网 13 包），
/// 但**旧档那 56 个包官网全都没有**（官网删掉了早期版本），所以旧档数据
/// 必须完整保留在 catalog 里——只是默认不跟官网那条并列显示。
///
/// 只作用于同属 FLiNG 的两个来源；小幸是独立作者，同名游戏照常并列展示。
/// 已知「同一份修改器覆盖多个游戏名」的等价组（组内第一个是代表名）。
///
/// 用于跨来源去重时把不同写法认作同一游戏。只登记人工确认过的关系，
/// 依据见 scripts/trainers/overrides.json。
const NAME_EQUIVALENTS: &[&[&str]] = &[
    // fling 官网把本体与冰原合并成一条（包文件名是 Monster Hunter World
    // v20200109-...，不含 Iceborne），旧档另有一条本体。
    &["monsterhunterworldiceborne", "monsterhunterworld"],
];

/// 归一化后再按等价组合并成代表名，用作去重键。
fn canonical_name(game_name: &str) -> String {
    let n = normalize_name(game_name);
    for group in NAME_EQUIVALENTS {
        if group.iter().any(|g| *g == n) {
            return group[0].to_string();
        }
    }
    n
}

pub fn dedupe_fling_family(hits: Vec<TrainerHit>) -> Vec<TrainerHit> {
    let official: std::collections::HashSet<String> = hits
        .iter()
        .filter(|h| h.source_id == "fling")
        .map(|h| canonical_name(&h.game_name))
        .collect();
    if official.is_empty() {
        return hits;
    }
    hits.into_iter()
        .filter(|h| {
            !(h.source_id == "fling_archive" && official.contains(&canonical_name(&h.game_name)))
        })
        .collect()
}

pub fn normalize_name(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut digits = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        if !digits.is_empty() {
            out.push_str(&fuzzy::to_roman(&digits));
            digits.clear();
        }
        if c.is_ascii_alphabetic() || is_cjk_ideograph(c) {
            out.extend(c.to_lowercase());
        } else if let Some(base) = fold_diacritics(c) {
            // 带变音符的拉丁字母折叠成 ASCII 基字母（ö→o）。
            // 旧行为是把非 ASCII 字母直接丢弃：Ragnarök 归一化成 ragnarrk，
            // 与 fling 的 Ragnarok（ragnarok）永远对不上，修改器拿不到官方中文名。
            out.push_str(&base.to_lowercase());
        }
    }
    if !digits.is_empty() {
        out.push_str(&fuzzy::to_roman(&digits));
    }
    out
}

fn is_cjk_ideograph(c: char) -> bool {
    matches!(c,
        '\u{3400}'..='\u{4DBF}'   // CJK 扩展 A
        | '\u{4E00}'..='\u{9FFF}' // CJK 基本区
        | '\u{F900}'..='\u{FAFF}' // 兼容表意文字
    )
}

/// 拉丁变音折叠：把带变音符的拉丁字母映射为 ASCII 基字母（ö→o、é→e、ß→ss）。
/// 覆盖 Latin-1 Supplement、Latin Extended-A、Latin Extended-Additional，
/// 以及 NFD 不分解的约定等价（ø、đ、ł、ŧ、ħ、æ、œ、ß）。
/// 表由 Python `unicodedata`（NFD 分解取基字母）生成，勿手工增删单条。
fn fold_diacritics(c: char) -> Option<&'static str> {
    Some(match c {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' | 'Ḁ' | 'Ạ' | 'Ả' | 'Ấ' | 'Ầ' | 'Ẩ' | 'Ẫ' | 'Ậ' | 'Ắ' | 'Ằ' | 'Ẳ' | 'Ẵ' | 'Ặ' => "A",
        'Æ' => "AE",
        'Ḃ' | 'Ḅ' | 'Ḇ' => "B",
        'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' | 'Ḉ' => "C",
        'Ď' | 'Đ' | 'Ḋ' | 'Ḍ' | 'Ḏ' | 'Ḑ' | 'Ḓ' => "D",
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' | 'Ḕ' | 'Ḗ' | 'Ḙ' | 'Ḛ' | 'Ḝ' | 'Ẹ' | 'Ẻ' | 'Ẽ' | 'Ế' | 'Ề' | 'Ể' | 'Ễ' | 'Ệ' => "E",
        'Ḟ' => "F",
        'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' | 'Ḡ' => "G",
        'Ĥ' | 'Ħ' | 'Ḣ' | 'Ḥ' | 'Ḧ' | 'Ḩ' | 'Ḫ' => "H",
        'Ì' | 'Í' | 'Î' | 'Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' | 'Ḭ' | 'Ḯ' | 'Ỉ' | 'Ị' => "I",
        'Ĵ' => "J",
        'Ķ' | 'Ḱ' | 'Ḳ' | 'Ḵ' => "K",
        'Ĺ' | 'Ļ' | 'Ľ' | 'Ł' | 'Ḷ' | 'Ḹ' | 'Ḻ' | 'Ḽ' => "L",
        'Ḿ' | 'Ṁ' | 'Ṃ' => "M",
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' | 'Ṅ' | 'Ṇ' | 'Ṉ' | 'Ṋ' => "N",
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ŏ' | 'Ő' | 'Ṍ' | 'Ṏ' | 'Ṑ' | 'Ṓ' | 'Ọ' | 'Ỏ' | 'Ố' | 'Ồ' | 'Ổ' | 'Ỗ' | 'Ộ' | 'Ớ' | 'Ờ' | 'Ở' | 'Ỡ' | 'Ợ' => "O",
        'Œ' => "OE",
        'Ṕ' | 'Ṗ' => "P",
        'Ŕ' | 'Ŗ' | 'Ř' | 'Ṙ' | 'Ṛ' | 'Ṝ' | 'Ṟ' => "R",
        'Ś' | 'Ŝ' | 'Ş' | 'Š' | 'Ṡ' | 'Ṣ' | 'Ṥ' | 'Ṧ' | 'Ṩ' => "S",
        'Ţ' | 'Ť' | 'Ŧ' | 'Ṫ' | 'Ṭ' | 'Ṯ' | 'Ṱ' => "T",
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' | 'Ṳ' | 'Ṵ' | 'Ṷ' | 'Ṹ' | 'Ṻ' | 'Ụ' | 'Ủ' | 'Ứ' | 'Ừ' | 'Ử' | 'Ữ' | 'Ự' => "U",
        'Ṽ' | 'Ṿ' => "V",
        'Ŵ' | 'Ẁ' | 'Ẃ' | 'Ẅ' | 'Ẇ' | 'Ẉ' => "W",
        'Ẋ' | 'Ẍ' => "X",
        'Ý' | 'Ŷ' | 'Ÿ' | 'Ẏ' | 'Ỳ' | 'Ỵ' | 'Ỷ' | 'Ỹ' => "Y",
        'Ź' | 'Ż' | 'Ž' | 'Ẑ' | 'Ẓ' | 'Ẕ' => "Z",
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'ḁ' | 'ạ' | 'ả' | 'ấ' | 'ầ' | 'ẩ' | 'ẫ' | 'ậ' | 'ắ' | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' => "a",
        'æ' => "ae",
        'ḃ' | 'ḅ' | 'ḇ' => "b",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' | 'ḉ' => "c",
        'ď' | 'đ' | 'ḋ' | 'ḍ' | 'ḏ' | 'ḑ' | 'ḓ' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' | 'ḕ' | 'ḗ' | 'ḙ' | 'ḛ' | 'ḝ' | 'ẹ' | 'ẻ' | 'ẽ' | 'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' => "e",
        'ḟ' => "f",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' | 'ḡ' => "g",
        'ĥ' | 'ħ' | 'ḣ' | 'ḥ' | 'ḧ' | 'ḩ' | 'ḫ' | 'ẖ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ḭ' | 'ḯ' | 'ỉ' | 'ị' => "i",
        'ĵ' => "j",
        'ķ' | 'ḱ' | 'ḳ' | 'ḵ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ł' | 'ḷ' | 'ḹ' | 'ḻ' | 'ḽ' => "l",
        'ḿ' | 'ṁ' | 'ṃ' => "m",
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ṅ' | 'ṇ' | 'ṉ' | 'ṋ' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'ṍ' | 'ṏ' | 'ṑ' | 'ṓ' | 'ọ' | 'ỏ' | 'ố' | 'ồ' | 'ổ' | 'ỗ' | 'ộ' | 'ớ' | 'ờ' | 'ở' | 'ỡ' | 'ợ' => "o",
        'œ' => "oe",
        'ṕ' | 'ṗ' => "p",
        'ŕ' | 'ŗ' | 'ř' | 'ṙ' | 'ṛ' | 'ṝ' | 'ṟ' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ṡ' | 'ṣ' | 'ṥ' | 'ṧ' | 'ṩ' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' | 'ṫ' | 'ṭ' | 'ṯ' | 'ṱ' | 'ẗ' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'ṳ' | 'ṵ' | 'ṷ' | 'ṹ' | 'ṻ' | 'ụ' | 'ủ' | 'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự' => "u",
        'ṽ' | 'ṿ' => "v",
        'ŵ' | 'ẁ' | 'ẃ' | 'ẅ' | 'ẇ' | 'ẉ' | 'ẘ' => "w",
        'ẋ' | 'ẍ' => "x",
        'ý' | 'ÿ' | 'ŷ' | 'ẏ' | 'ẙ' | 'ỳ' | 'ỵ' | 'ỷ' | 'ỹ' => "y",
        'ź' | 'ż' | 'ž' | 'ẑ' | 'ẓ' | 'ẕ' => "z",
        _ => return None,
    })
}

/// 版本后缀 · **可靠档**：同一套程序加内容（DLC/季票），修改器通常通用。
///
/// 顺序很重要：多词的放前面，先剥长的（`completeedition` 先于 `complete`）。
const EDITION_SUFFIXES_SAFE: &[&str] = &[
    "gameoftheyearedition",
    "digitaldeluxeedition",
    "collectorsedition",
    "anniversaryedition",
    "definitiveedition",
    "legendaryedition",
    "completeedition",
    "ultimateedition",
    "enhancededition",
    "specialedition",
    "deluxeedition",
    "goldedition",
    "gotyedition",
    "reignofsand",
];

/// 版本后缀 · **兜底档**：重新编译过的另一个程序（重制/导演剪辑/增强），
/// 原版修改器很可能注入失败。只在可靠档完全没命中时才采用，并标记 `edition_fallback`。
const EDITION_SUFFIXES_FALLBACK: &[&str] = &[
    "directorscut",
    "remastered",
    "enhanced",
    "complete",
    "ultimate",
    "deluxe",
];

/// 剥掉结尾的版本后缀。`list` 决定用哪一档（可靠 / 兜底 / 两者）。
fn strip_with<'a>(normalized: &'a str, list: &[&str]) -> &'a str {
    let mut current = normalized;
    'outer: loop {
        for suffix in list {
            if let Some(head) = current.strip_suffix(suffix) {
                if head.chars().count() >= 4 {
                    current = head;
                    continue 'outer;
                }
            }
        }
        return current;
    }
}

/// 剥掉结尾的版本后缀（两档合并），只用于**匹配比较**，不改显示名。
/// 剩余部分不足 4 个字符就停手，避免把 `Deluxe` 这种就是名字本身的标题剥成空。
pub fn strip_edition_suffix(normalized: &str) -> &str {
    let once = strip_with(normalized, EDITION_SUFFIXES_SAFE);
    strip_with(once, EDITION_SUFFIXES_FALLBACK)
}

/// 只剥可靠档（内容包类）
fn strip_safe(normalized: &str) -> &str {
    strip_with(normalized, EDITION_SUFFIXES_SAFE)
}

/// 只剥兜底档（重制/增强类）
fn strip_fallback(normalized: &str) -> &str {
    strip_with(normalized, EDITION_SUFFIXES_FALLBACK)
}

// ======================================================================
// 中文名索引（游戏库 title → zh_CN 回填）
// ======================================================================

/// 归一化英文名 → 中文名。数据来自游戏库（见 database::Database::title_zh_pairs）。
/// 检索时给条目回填 zhName，也让中文输入能命中目录条目。
pub struct ZhNameIndex {
    map: HashMap<String, String>,
    /// 归一化英文名 → 官方繁体名（繁体界面用，缺省为空）
    hant: HashMap<String, String>,
}

impl ZhNameIndex {
    pub fn build<I: IntoIterator<Item = (String, Option<String>)>>(rows: I) -> Self {
        Self::build_full(rows.into_iter().map(|(t, z)| (t, z, None)))
    }

    /// 同时建立简中与繁中两条索引
    pub fn build_full<I: IntoIterator<Item = (String, Option<String>, Option<String>)>>(
        rows: I,
    ) -> Self {
        let mut map = HashMap::new();
        let mut hant = HashMap::new();
        for (title, zh, h) in rows {
            let key = normalize_name(&title);
            if let Some(zh) = zh {
                if !zh.trim().is_empty() {
                    map.entry(key.clone()).or_insert(zh);
                }
            }
            if let Some(h) = h {
                if !h.trim().is_empty() {
                    hant.entry(key).or_insert(h);
                }
            }
        }
        ZhNameIndex { map, hant }
    }

    pub fn empty() -> Self {
        ZhNameIndex {
            map: HashMap::new(),
            hant: HashMap::new(),
        }
    }

    /// 游戏英文名 → 游戏库中文名（精确归一化匹配）
    pub fn get(&self, game_name: &str) -> Option<&str> {
        self.map.get(&normalize_name(game_name)).map(String::as_str)
    }

    /// 游戏英文名 → 官方繁体名
    pub fn get_hant(&self, game_name: &str) -> Option<&str> {
        self.hant.get(&normalize_name(game_name)).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

// ======================================================================
// 来源 trait 与注册表
// ======================================================================

/// 一个修改器数据来源。实现后即可被注册表加载并参与统一检索。
pub trait TrainerSource: Send + Sync {
    fn info(&self) -> &TrainerSourceInfo;
    fn entries(&self) -> &[TrainerEntry];
}

/// 多来源注册表：聚合所有来源的检索与匹配。
#[derive(Default)]
pub struct TrainerRegistry {
    sources: Vec<Box<dyn TrainerSource>>,
}

impl TrainerRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    pub fn total(&self) -> usize {
        self.sources.iter().map(|s| s.entries().len()).sum()
    }

    /// 已加载来源的元信息（含条目数）
    pub fn source_infos(&self) -> Vec<(TrainerSourceInfo, usize)> {
        self.sources
            .iter()
            .map(|s| (s.info().clone(), s.entries().len()))
            .collect()
    }

    /// 按条目 id（`<source>/<slug>`）跨来源查找
    pub fn find_entry(&self, id: &str) -> Option<(&str, &TrainerEntry)> {
        for source in &self.sources {
            if let Some(entry) = source.entries().iter().find(|e| e.id == id) {
                return Some((source.info().id.as_str(), entry));
            }
        }
        None
    }

    /// 更新检查：已安装包的下载地址 ≠ 目录里该条目的最新包 → 可更新。
    /// 判定键是下载地址（站点没有独立版本号），同条目多包时只对最新包报告。
    pub fn check_updates(&self, root: &Path) -> Vec<TrainerUpdateInfo> {
        let mut updates = Vec::new();
        for installed in install::scan_installed(root) {
            let Some((_source_id, entry)) = self.find_entry(&installed.manifest.meta.trainer_id)
            else {
                continue; // 目录已不含此条目（下架）→ 不误报更新
            };
            let Some(latest) = entry.latest() else { continue };
            if latest.url != installed.manifest.meta.download_url {
                updates.push(TrainerUpdateInfo {
                    trainer_id: entry.id.clone(),
                    game_name: entry.game_name.clone(),
                    zh_name: entry.zh_name.clone(),
                    old_folder: installed.folder,
                    new_download: latest.clone(),
                });
            }
        }
        updates
    }

    /// 从单个目录加载全部 catalog 文件（每来源一份 *.json）。
    /// 目录不存在 → 空注册表；无法识别的 schema/来源跳过并记入错误列表（不阻断其他来源）。
    pub fn load_dir(dir: &Path) -> (Self, Vec<String>) {
        Self::load_dirs(&[dir.to_path_buf()])
    }

    /// 从多个目录按序加载；同一来源 id 出现多次时，**后面的目录覆盖前面的**
    /// （用于"内置目录 + 用户更新目录"的叠加：应用数据目录排最后，胜出）。
    pub fn load_dirs(dirs: &[std::path::PathBuf]) -> (Self, Vec<String>) {
        let mut sources: Vec<Box<dyn TrainerSource>> = Vec::new();
        let mut index_by_id: HashMap<String, usize> = HashMap::new();
        let mut errors = Vec::new();
        for dir in dirs {
            let Ok(files) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut paths: Vec<_> = files.flatten().map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if !name.to_lowercase().ends_with(".json") {
                    continue;
                }
                let loaded: Option<Box<dyn TrainerSource>> = match std::fs::read_to_string(&path)
                {
                    Ok(json) => match catalog_source_id(&json) {
                        Ok(id) => match id.as_str() {
                            fling::SOURCE_ID => match fling::FlingSource::from_json(&json) {
                                Ok(s) => Some(Box::new(s)),
                                Err(e) => {
                                    errors.push(format!("{name}: {e}"));
                                    None
                                }
                            },
                            FLING_ARCHIVE_SOURCE_ID | XIAOXING_SOURCE_ID => {
                                match catalog::CatalogSource::from_json(&json, &id) {
                                    Ok(s) => Some(Box::new(s)),
                                    Err(e) => {
                                        errors.push(format!("{name}: {e}"));
                                        None
                                    }
                                }
                            }
                            other => {
                                errors.push(format!("{name}: 未知的修改器来源「{other}」，已跳过"));
                                None
                            }
                        },
                        Err(e) => {
                            errors.push(format!("{name}: {e}"));
                            None
                        }
                    },
                    Err(e) => {
                        errors.push(format!("{name}: 读取失败 {e}"));
                        None
                    }
                };
                if let Some(source) = loaded {
                    let id = source.info().id.clone();
                    match index_by_id.get(&id) {
                        Some(&i) => sources[i] = source, // 后目录覆盖前目录
                        None => {
                            index_by_id.insert(id, sources.len());
                            sources.push(source);
                        }
                    }
                }
            }
        }
        (TrainerRegistry { sources }, errors)
    }

    /// 关键词检索全部来源。语义：
    /// - 归一化后做子串匹配（英文名、别名 + 已解析的中文名），
    ///   外加缩写 / 拼音桥接 / 模糊纠错三类召回（见 fuzzy.rs）
    /// - 归一化后不足 2 个字符的关键词一律无结果（防泛匹配刷屏）；
    ///   例外：单个 CJK 字符放行（中文名高度特异，"黑""仙"这类单字命中率低且是有用前缀）
    /// - 按相关度降序返回（全等 > 前缀 > 子串 > 缩写 > 拼音 > 模糊），
    ///   同分保持来源内名称序；先全量评分再截断，避免限量截掉高分结果
    pub fn search(&self, query: &str, zh: &ZhNameIndex, limit: usize) -> Vec<TrainerHit> {
        let matcher = fuzzy::QueryMatcher::new(query);
        // 单个 ASCII 字符仍拒绝（子串命中会刷屏）；单个 CJK 字符放行：
        // 中文名高度特异（"黑"→黑神话悟空），且 CJK 字符集远大于 ASCII，
        // 泛匹配风险低。normalized_chars 数的是字符不是字节，CJK 单字 = 1。
        let is_single_cjk = matcher.normalized_chars() == 1
            && matcher
                .normalized()
                .chars()
                .next()
                .is_some_and(crate::trainers::is_cjk_ideograph);
        if matcher.normalized_chars() < 2 && !is_single_cjk {
            return Vec::new();
        }
        let mut scored: Vec<(f64, TrainerHit)> = Vec::new();
        for source in &self.sources {
            for entry in source.entries() {
                let zh_name = entry
                    .zh_name
                    .as_deref()
                    .map(String::from)
                    .or_else(|| zh.get(&entry.game_name).map(String::from));
                let zh_disp = zh_name.as_deref();
                let hant = zh.get_hant(&entry.game_name);
                // 条目名与别名取最高相关度
                let mut score = matcher.match_score(&entry.game_name, zh_disp);
                if let Some(s) = matcher.match_score(&entry.game_name, hant) {
                    score = Some(score.map_or(s, |b| b.max(s)));
                }
                for a in &entry.aliases {
                    if let Some(s) = matcher.match_score(a, zh_disp) {
                        score = Some(score.map_or(s, |b| b.max(s)));
                    }
                    if let Some(s) = matcher.match_score(a, hant) {
                        score = Some(score.map_or(s, |b| b.max(s)));
                    }
                }
                if let Some(s) = score {
                    scored.push((
                        s,
                        TrainerHit::from_entry(&source.info().id, entry)
                            .with_zh_fallback(zh_name)
                            .with_zh_hant_fallback(
                                zh.get_hant(&entry.game_name).map(String::from),
                            ),
                    ));
                }
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        // 先去重再截断：去重会砍掉旧档的重复项，若先截断则结果会不足 limit 条
        let hits: Vec<TrainerHit> = scored.into_iter().map(|(_, h)| h).collect();
        dedupe_fling_family(hits).into_iter().take(limit).collect()
    }

    /// 已安装游戏 ↔ 修改器匹配。语义（见 SPEC.md）：归一化后**全等**才命中
    /// （不做子串：避免《DOOM》误配《DOOM Eternal》这类真实存在的另一款游戏），
    /// 版本变体通过条目别名登记（aliases 与 game_name 同权）。
    /// 双向比较英文名与中文名；每款游戏可命中多个来源的条目。
    pub fn match_games(
        &self,
        zh: &ZhNameIndex,
        games: &[(String, Option<String>)], // (title, zhName?)
    ) -> Vec<GameTrainerMatch> {
        games
            .iter()
            .map(|(title, game_zh)| {
                let n_title = normalize_name(title);
                let n_game_zh = game_zh.as_deref().map(normalize_name);
                let mut matches = Vec::new();
                let mut soft_matches = Vec::new();
                for source in &self.sources {
                    for entry in source.entries() {
                        // 显示用原始中文名；比较用归一化文本
                        let trainer_zh_display = entry
                            .zh_name
                            .clone()
                            .or_else(|| zh.get(&entry.game_name).map(String::from));
                        let trainer_zh_norm = trainer_zh_display.as_deref().map(normalize_name);
                        let entry_name = normalize_name(&entry.game_name);
                        let zh_hit = matches!( (&trainer_zh_norm, &n_game_zh),
                            (Some(a), Some(b)) if !b.is_empty() && a == b );
                        // 全等（或命中别名）
                        let exact = entry_name == n_title
                            || entry
                                .aliases
                                .iter()
                                .any(|a| normalize_name(a) == n_title);
                        // 可靠档：只差内容包后缀（Complete / Ultimate / GOTY / Reign of Sand …）
                        let safe_hit = !exact
                            && strip_safe(&entry_name) == strip_safe(&n_title);
                        // 兜底档：只差重制类后缀（Remastered / Director's Cut / Enhanced …）
                        let soft_hit = !exact
                            && !safe_hit
                            && strip_fallback(&entry_name) == strip_fallback(&n_title);

                        if exact || safe_hit || zh_hit {
                            matches.push(
                                TrainerHit::from_entry(&source.info().id, entry)
                                    .with_zh_fallback(
                                        trainer_zh_display.or_else(|| game_zh.clone()),
                                    )
                                    .with_zh_hant_fallback(
                                        zh.get_hant(&entry.game_name).map(String::from),
                                    ),
                            );
                        } else if soft_hit {
                            let mut hit = TrainerHit::from_entry(&source.info().id, entry)
                                .with_zh_fallback(trainer_zh_display.or_else(|| game_zh.clone()))
                                .with_zh_hant_fallback(
                                    zh.get_hant(&entry.game_name).map(String::from),
                                );
                            hit.edition_fallback = true;
                            soft_matches.push(hit);
                        }
                    }
                }
                // 兜底档只在可靠档完全没命中时才采用：宁可少匹配，也不让重制版的修改器
                // 冒充原版。采用时条目带 edition_fallback 标记，前端可以提示"版本可能不同"。
                if matches.is_empty() {
                    matches = soft_matches;
                }
                GameTrainerMatch {
                    title: title.clone(),
                    zh_name: game_zh.clone(),
                    matches: dedupe_fling_family(matches),
                }
            })
            .collect()
    }
}

/// 轻量读 catalog 信封，仅取 schema 与 source.id 用于分发（完整校验在各来源实现里）
fn catalog_source_id(json: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let schema = value
        .get("schema")
        .and_then(|v| v.as_str())
        .ok_or("缺少 schema 字段")?;
    if schema != CATALOG_SCHEMA {
        return Err(format!("不支持的 catalog 格式 {schema}（需要 {CATALOG_SCHEMA}）"));
    }
    value
        .get("source")
        .and_then(|s| s.get("id"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or("缺少 source.id 字段".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn zh_index() -> ZhNameIndex {
        ZhNameIndex::build([
            ("Black Myth: Wukong".to_string(), Some("黑神话：悟空".to_string())),
            ("ELDEN RING".to_string(), Some("艾尔登法环".to_string())),
            ("No Zh Game".to_string(), None),
        ])
    }

    fn registry() -> TrainerRegistry {
        let json = r#"{
            "schema": "gm.trainers/1",
            "generatedAt": "2026-10-01T00:00:00Z",
            "source": { "id": "fling", "title": "FLiNG Trainer", "homepage": "https://flingtrainer.com" },
            "trainers": [
                { "id": "fling/black-myth-wukong-trainer", "slug": "black-myth-wukong-trainer",
                  "gameName": "Black Myth: Wukong", "pageUrl": "https://flingtrainer.com/trainer/black-myth-wukong-trainer/",
                  "updatedAt": "2025-10-17",
                  "downloads": [ { "file": "B.v1.0-v1.0.20.Plus.44.Trainer-FLiNG", "url": "https://x/d1",
                                   "sizeBytes": 944128, "downloadCount": 1311424, "gameVersion": "1.0-1.0.20", "optionsCount": 44 },
                                 { "file": "B.v1.0-v1.0.13.Plus.44.Trainer-FLiNG", "url": "https://x/d2" } ] },
                { "id": "fling/elden-ring-trainer", "slug": "elden-ring-trainer",
                  "gameName": "ELDEN RING", "pageUrl": "https://flingtrainer.com/trainer/elden-ring-trainer/",
                  "downloads": [ { "file": "E.v1.0.Plus.19.Trainer-FLiNG", "url": "https://x/d3", "optionsCount": 19 } ] },
                { "id": "fling/doom-trainer", "slug": "doom-trainer",
                  "gameName": "DOOM", "pageUrl": "https://flingtrainer.com/trainer/doom-trainer/", "downloads": [] }
            ]
        }"#;
        let source = fling::FlingSource::from_json(json).unwrap();
        let mut reg = TrainerRegistry::empty();
        reg.sources.push(Box::new(source));
        reg
    }

    #[test]
    fn normalize_keeps_alnum_and_cjk_drops_punct() {
        assert_eq!(normalize_name("Black Myth: Wukong"), "blackmythwukong");
        assert_eq!(normalize_name("ELDEN RING"), "eldenring");
        assert_eq!(normalize_name("艾尔登法环"), "艾尔登法环");
        assert_eq!(normalize_name("Assassin's Creed® Valhalla"), "assassinscreedvalhalla");
        assert_eq!(normalize_name("黑神话：悟空"), "黑神话悟空");
    }

    #[test]
    fn diacritics_fold_to_ascii_base() {
        // fling 把 Ragnarök ASCII 化成 Ragnarok，游戏库保留 ö；
        // 旧逻辑把 ö 丢弃 → ragnarrk ≠ ragnarok，修改器匹配不到游戏、拿不到官方中文名。
        assert_eq!(normalize_name("God of War Ragnarök"), normalize_name("God of War Ragnarok"));
        assert_eq!(normalize_name("God of War Ragnarök"), "godofwarragnarok");
        assert_eq!(normalize_name("Pokémon"), normalize_name("Pokemon"));
        assert_eq!(normalize_name("Café"), "cafe");
        assert_eq!(normalize_name("Brütal Legend"), normalize_name("Brutal Legend"));
        assert_eq!(normalize_name("Æon Flux"), normalize_name("Aeon Flux"));
        assert_eq!(normalize_name("Façade"), "facade");
        assert_eq!(normalize_name("Motörhead"), "motorhead");
        // 大小写无关：大写变音字母同样折叠
        assert_eq!(normalize_name("WARRIORS OF RAGNARÖK"), normalize_name("Warriors of Ragnarok"));
    }

    #[test]
    #[ignore = "读真实数据库与目录，手动跑：cargo test -p gm-core match_report -- --ignored --nocapture"]
    fn match_report_against_real_data() {
        use std::collections::HashSet;

        let src_tau = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let trainers_dir = src_tau.join("resources").join("trainers");
        let db_path = src_tau.join("resources").join("database.db");

        let (registry, errors) = TrainerRegistry::load_dir(&trainers_dir);
        if !errors.is_empty() {
            println!("加载告警: {errors:?}");
        }
        let db = crate::database::Database::open_readonly(&db_path).expect("打开数据库失败");
        let games = db.title_zh_pairs().expect("读取游戏列表失败");
        let zh = ZhNameIndex::build(games.iter().cloned());

        let matched = registry.match_games(&zh, &games);
        let with_hit = matched.iter().filter(|m| !m.matches.is_empty()).count();
        let total_hits: usize = matched.iter().map(|m| m.matches.len()).sum();
        let soft: usize = matched
            .iter()
            .flat_map(|m| m.matches.iter())
            .filter(|h| h.edition_fallback)
            .count();

        // 目录侧覆盖率：有多少条修改器至少挂上了一款游戏
        let mut attached: HashSet<(String, String)> = HashSet::new();
        for m in &matched {
            for h in &m.matches {
                attached.insert((h.source_id.clone(), h.id.clone()));
            }
        }
        let all_entries: usize = registry.sources.iter().map(|s| s.entries().len()).sum();

        println!("游戏总数            {}", games.len());
        println!("有修改器的游戏      {with_hit}");
        println!("命中条目总数        {total_hits}（其中兜底档 {soft}）");
        println!("目录条目总数        {all_entries}");
        println!("已挂到游戏的条目    {}", attached.len());
        println!("未挂到任何游戏的    {}", all_entries - attached.len());
    }

    #[test]
    fn strip_edition_suffix_handles_official_titles() {
        // Steam 官方名带版本后缀，修改器目录用原始名 —— 剥掉后应当相等
        assert_eq!(
            strip_edition_suffix(&normalize_name("Atlas Fallen: Reign Of Sand")),
            strip_edition_suffix(&normalize_name("Atlas Fallen"))
        );
        assert_eq!(
            strip_edition_suffix(&normalize_name("Godfall Ultimate Edition")),
            strip_edition_suffix(&normalize_name("Godfall"))
        );
        assert_eq!(
            strip_edition_suffix(&normalize_name("Chernobylite Complete Edition")),
            strip_edition_suffix(&normalize_name("Chernobylite"))
        );
        assert_eq!(
            strip_edition_suffix(&normalize_name("Grand Theft Auto V Enhanced")),
            strip_edition_suffix(&normalize_name("Grand Theft Auto V"))
        );
        // 不能误伤：这是两款不同游戏
        assert_ne!(
            strip_edition_suffix(&normalize_name("DOOM Eternal")),
            strip_edition_suffix(&normalize_name("DOOM"))
        );
        assert_ne!(
            strip_edition_suffix(&normalize_name("HITMAN 2: Silent Assassin")),
            strip_edition_suffix(&normalize_name("HITMAN 2"))
        );
        // 剩余太短则不剥（防止 Deluxe 这类本身就是名字）
        assert_eq!(strip_edition_suffix("deluxe"), "deluxe");
    }

    /// 造一个最小 TrainerHit 用于去重测试
    fn hit(source: &str, name: &str) -> TrainerHit {
        TrainerHit {
            source_id: source.into(),
            id: format!("{source}/x"),
            game_name: name.into(),
            zh_name: None,
            zh_hant: None,
            official_name: None,
            page_url: String::new(),
            updated_at: None,
            options_count: None,
            game_version: None,
            download_url: None,
            external_url: None,
            size_bytes: None,
            download_count: None,
            total_downloads: 0,
            edition_fallback: false,
        }
    }

    /// 历史版本不能丢：官网有条目时隐藏旧档同游戏那条，
    /// 但旧档独有的游戏、以及小幸（独立作者）的同名条目都要保留。
    #[test]
    fn dedupe_hides_archive_only_when_official_has_same_game() {
        let hits = vec![
            hit("fling", "Anno 1800"),
            hit("fling_archive", "Anno 1800"),
            hit("fling_archive", "Alan Wake"),
            hit("xiaoxing", "Anno 1800"),
        ];
        let out = dedupe_fling_family(hits);
        assert_eq!(out.len(), 3, "应只砍掉旧档那条重复：{out:?}");
        assert!(out.iter().any(|h| h.source_id == "fling"));
        assert!(!out
            .iter()
            .any(|h| h.source_id == "fling_archive" && h.game_name == "Anno 1800"));
        assert!(out
            .iter()
            .any(|h| h.source_id == "fling_archive" && h.game_name == "Alan Wake"));
        assert!(out.iter().any(|h| h.source_id == "xiaoxing"), "小幸是独立作者，不能去重");
    }

    /// 归一化后同名（大小写/标点差异）也要认作同一游戏
    #[test]
    fn dedupe_treats_normalized_names_as_same_game() {
        let hits = vec![
            hit("fling", "ARK: Survival Evolved"),
            hit("fling_archive", "ARK Survival Evolved"),
        ];
        assert_eq!(dedupe_fling_family(hits).len(), 1);
    }

    #[test]
    fn search_matches_case_punct_insensitive_and_zh() {
        let reg = registry();
        let zh = zh_index();
        assert_eq!(reg.search("black myth wukong", &zh, 50).len(), 1);
        assert_eq!(reg.search("BlackMyth:WUKONG", &zh, 50).len(), 1);
        assert_eq!(reg.search("艾尔登法环", &zh, 50).len(), 1); // 中文经索引命中英文名条目
        assert_eq!(reg.search("黑神话：悟空", &zh, 50).len(), 1); // 中文经回填 zhName 命中
        assert!(reg.search("unreal", &zh, 50).is_empty());
    }

    #[test]
    fn search_rejects_too_short_queries() {
        let reg = registry();
        assert!(reg.search("d", &ZhNameIndex::empty(), 50).is_empty());
        assert!(reg.search("", &ZhNameIndex::empty(), 50).is_empty());
        // 归一化后剩 1 个字符的符号串同样拒绝
        assert!(reg.search("!!", &ZhNameIndex::empty(), 50).is_empty());
    }

    #[test]
    fn search_scores_traditional_names_before_filtering_and_limiting() {
        let zh = ZhNameIndex::build_full(vec![(
            "Elden Ring".to_string(), Some("艾尔登法环".to_string()),
            Some("艾爾登法環".to_string()),
        )]);
        let reg = registry();
        let hits = reg.search("艾爾登法環", &zh, 1);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "fling/elden-ring-trainer");
        assert_eq!(hits[0].zh_hant.as_deref(), Some("艾爾登法環"));
        assert_eq!(reg.search("艾尔登法环", &zh, 1).len(), 1);
        assert_eq!(reg.search("elden ring", &zh, 1).len(), 1);
    }

    #[test]
    fn search_limit_and_hit_shape() {
        let reg = registry();
        let zh = zh_index();
        let hits = reg.search("ring", &zh, 50);
        assert_eq!(hits.len(), 1);
        let hit = &hits[0];
        assert_eq!(hit.id, "fling/elden-ring-trainer");
        assert_eq!(hit.source_id, "fling");
        assert_eq!(hit.zh_name.as_deref(), Some("艾尔登法环"));
        assert_eq!(hit.options_count, Some(19));
        assert_eq!(hit.total_downloads, 1);

        let limited = reg.search("o", &zh, 50); // 归一化 1 字符 → 拒绝
        assert!(limited.is_empty());
        let all = reg.search("black", &zh, 50);
        assert_eq!(all[0].total_downloads, 2);
    }

    #[test]
    fn match_games_requires_exact_normalized_equality() {
        let reg = registry();
        let zh = zh_index();
        let games = vec![
            ("Black Myth: Wukong".to_string(), None),
            ("ELDEN RING".to_string(), Some("艾尔登法环".to_string())),
            ("blackmythwukong".to_string(), None), // 归一化后等价也算命中
            ("DOOM Eternal".to_string(), None),    // 不能误配 DOOM
            ("Not Installed Game".to_string(), None),
        ];
        let out = reg.match_games(&zh, &games);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].matches.len(), 1);
        assert_eq!(out[0].matches[0].id, "fling/black-myth-wukong-trainer");
        assert_eq!(out[1].matches.len(), 1); // 英文归一化相等（大小写）+ 中文相等双保险
        assert_eq!(out[2].matches.len(), 1); // 传入已归一化的标题也能命中
        assert!(out[3].matches.is_empty());
        assert!(out[4].matches.is_empty());
    }

    #[test]
    fn match_games_and_search_honor_aliases() {
        // 版本变体：条目 "Monster Hunter World: Iceborne" 登记基础版别名
        let json = r#"{
            "schema": "gm.trainers/1",
            "source": { "id": "fling", "title": "FLiNG", "homepage": "https://x" },
            "trainers": [
                { "id": "fling/mhw-iceborne", "slug": "mhw-iceborne",
                  "gameName": "Monster Hunter World: Iceborne",
                  "aliases": ["Monster Hunter: World"],
                  "pageUrl": "https://flingtrainer.com/trainer/mhw-iceborne/",
                  "downloads": [ { "file": "MHW.IB.Plus.30.Trainer-FLiNG", "url": "https://x/mhw" } ] }
            ]
        }"#;
        let source = fling::FlingSource::from_json(json).unwrap();
        let mut reg = TrainerRegistry::empty();
        reg.sources.push(Box::new(source));
        let zh = ZhNameIndex::empty();

        // 匹配：基础版标题经别名命中；DOOM 式误配仍被拒
        let out = reg.match_games(
            &zh,
            &[
                ("Monster Hunter: World".to_string(), None),
                ("Monster Hunter Wilds".to_string(), None),
            ],
        );
        assert_eq!(out[0].matches.len(), 1, "基础版标题应经别名命中");
        assert_eq!(out[0].matches[0].id, "fling/mhw-iceborne");
        assert!(out[1].matches.is_empty(), "前缀相近的另一款游戏不得误配");

        // 检索：别名也参与子串匹配
        assert_eq!(reg.search("monster hunter: world", &zh, 50).len(), 1);
    }

    #[test]
    fn single_cjk_query_passes_gate() {
        // 戢翎 2026-11-03：搜"黑"要能出黑神话悟空（单 CJK 字符不再被最短长度拒绝）
        let reg = registry();
        let idx = zh_index();
        let hits = reg.search("黑", &idx, 20);
        assert!(
            hits.iter().any(|h| h.game_name.to_lowercase().contains("black myth")),
            "单字「黑」应命中 Black Myth: Wukong，实际: {:?}",
            hits.iter().map(|h| &h.game_name).collect::<Vec<_>>()
        );
        // 单个 ASCII 字符仍应被拒绝（防泛匹配）
        let hits_a = reg.search("z", &idx, 20);
        assert!(hits_a.is_empty(), "单 ASCII 字符应无结果");
    }

    #[test]
    fn zh_index_exact_normalized_lookup() {
        let zh = zh_index();
        assert_eq!(zh.get("Black Myth: Wukong"), Some("黑神话：悟空"));
        assert_eq!(zh.get("black   myth-wukong"), Some("黑神话：悟空"));
        assert_eq!(zh.get("Elden Ring"), Some("艾尔登法环"));
        assert_eq!(zh.get("unknown"), None);
        assert_eq!(zh.len(), 2); // zh 为 None 的行不进索引
    }

    #[test]
    fn hit_zh_fallback_prefers_existing() {
        let reg = registry();
        let zh = zh_index();
        let hit = &reg.search("wukong", &zh, 50)[0];
        assert_eq!(hit.zh_name.as_deref(), Some("黑神话：悟空"));
        // 游戏库没有的名字 → zhName 保持 None
        let hit = &reg.search("doom", &ZhNameIndex::empty(), 50)[0];
        assert_eq!(hit.zh_name, None);
        assert_eq!(hit.total_downloads, 0); // 无附件条目也能作为结果返回
    }

    #[test]
    fn load_dirs_later_dir_overrides_same_source() {
        let tmp = std::env::temp_dir().join(format!("gm-trainers-ovr-{}", std::process::id()));
        let d1 = tmp.join("a");
        let d2 = tmp.join("b");
        std::fs::create_dir_all(&d1).unwrap();
        std::fs::create_dir_all(&d2).unwrap();
        let entry = |name: &str| {
            format!(
                r#"{{"id":"fling/{name}","slug":"{name}","gameName":"{name}","pageUrl":"https://flingtrainer.com/trainer/{name}/"}}"#
            )
        };
        let catalog = |trainers: String| {
            format!(
                r#"{{"schema":"gm.trainers/1","source":{{"id":"fling","title":"FLiNG","homepage":"https://x"}},"trainers":[{trainers}]}}"#
            )
        };
        // 目录 A：旧版目录（两个条目）；目录 B：更新后的目录（一个条目）→ B 胜出
        std::fs::write(d1.join("fling.json"), catalog(format!("{},{}", entry("old1"), entry("old2")))).unwrap();
        std::fs::write(d2.join("fling.json"), catalog(entry("new1"))).unwrap();

        let (reg, errs) = TrainerRegistry::load_dirs(&[d1, d2]);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(reg.total(), 1);
        assert_eq!(reg.search("new1", &ZhNameIndex::empty(), 10).len(), 1);
        assert!(reg.search("old1", &ZhNameIndex::empty(), 10).is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn check_updates_and_delete_installed() {
        let reg = registry();
        let root = std::env::temp_dir().join(format!("gm-trainers-upd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let manifest_json = |url: &str| {
            format!(
                r#"{{"schema":"gm.trainer-manifest/1","trainerId":"fling/black-myth-wukong-trainer",
                    "sourceId":"fling","gameName":"Black Myth: Wukong","file":"B.Trainer-FLiNG",
                    "downloadUrl":"{url}","pageUrl":"https://flingtrainer.com/trainer/x/",
                    "installedAt":"2026-10-01T00:00:00Z","files":[]}}"#
            )
        };
        // 旧包（装的是 d2）→ 目录最新是 d1 → 可更新；再装一个"最新"的 → 不可更新
        let old_pkg = root.join("Black Myth Wukong").join("pkg-old");
        std::fs::create_dir_all(&old_pkg).unwrap();
        std::fs::write(old_pkg.join("manifest.json"), manifest_json("https://x/d2")).unwrap();
        let new_pkg = root.join("Black Myth Wukong").join("pkg-new");
        std::fs::create_dir_all(&new_pkg).unwrap();
        std::fs::write(new_pkg.join("manifest.json"), manifest_json("https://x/d1")).unwrap();

        let updates = reg.check_updates(&root);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].trainer_id, "fling/black-myth-wukong-trainer");
        assert!(updates[0].old_folder.ends_with("pkg-old"));
        assert_eq!(updates[0].new_download.url, "https://x/d1");

        // 删除安全约束：根外目录拒绝；无 manifest 拒绝；合法删除成功
        let outside = std::env::temp_dir().join(format!("gm-trainers-out-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("manifest.json"), manifest_json("https://x/d1")).unwrap();
        assert!(install::delete_installed(&root, &outside).is_err());
        let _ = std::fs::remove_dir_all(&outside);

        let no_manifest = root.join("Black Myth Wukong").join("empty-dir");
        std::fs::create_dir_all(&no_manifest).unwrap();
        assert!(install::delete_installed(&root, &no_manifest).is_err());

        assert!(install::delete_installed(&root, &old_pkg).is_ok());
        assert!(!old_pkg.exists());
        let updates = reg.check_updates(&root);
        assert!(updates.is_empty(), "删掉旧包后不应再报更新");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn check_updates_skips_missing_entries() {
        // 已安装条目在目录中不存在（下架）→ 不报更新也不报错
        let reg = registry();
        let root = std::env::temp_dir().join(format!("gm-trainers-gone-{}", std::process::id()));
        let pkg = root.join("Gone Game").join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(
            pkg.join("manifest.json"),
            r#"{"schema":"gm.trainer-manifest/1","trainerId":"fling/removed-trainer",
                "sourceId":"fling","gameName":"Removed Game","file":"R.Trainer-FLiNG",
                "downloadUrl":"https://x/gone","pageUrl":"https://flingtrainer.com/trainer/x/",
                "installedAt":"2026-10-01T00:00:00Z","files":[]}"#,
        )
        .unwrap();
        assert!(reg.check_updates(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_dir_handles_missing_dir_and_rejects_unknown_schema() {
        let (reg, errs) = TrainerRegistry::load_dir(Path::new("Z:/definitely/not/here"));
        assert!(reg.is_empty());
        assert!(errs.is_empty()); // 目录缺失不算错误，应用静默降级

        let tmp = std::env::temp_dir().join(format!("gm-trainers-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("bad.json"), r#"{"schema":"gm.trainers/9","source":{"id":"x"}}"#).unwrap();
        std::fs::write(tmp.join("not-catalog.json"), "[]").unwrap();
        let (reg, errs) = TrainerRegistry::load_dir(&tmp);
        assert!(reg.is_empty());
        assert_eq!(errs.len(), 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 真实打包资源冒烟：管道产出的 fling.json 能被引擎原样加载（数据库测试同款模式）
    #[test]
    fn real_catalog_loads_and_searches() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/trainers");
        let (reg, errs) = TrainerRegistry::load_dir(&dir);
        assert!(errs.is_empty(), "catalog 加载报错: {errs:?}");
        assert!(reg.total() >= 700, "条目异常少: {}", reg.total());
        let ids: Vec<_> = reg
            .source_infos()
            .into_iter()
            .map(|(info, _)| info.id)
            .collect();
        assert!(ids.contains(&fling::SOURCE_ID.to_string()));
        assert!(ids.contains(&FLING_ARCHIVE_SOURCE_ID.to_string()));
        assert!(ids.contains(&XIAOXING_SOURCE_ID.to_string()));

        let zh = ZhNameIndex::empty();
        let hits = reg.search("wukong", &zh, 50);
        assert!(hits.iter().any(|h| h.game_name.contains("Wukong")), "黑神话未命中");
        let hits = reg.search("elden", &zh, 50);
        assert!(hits.iter().any(|h| normalize_name(&h.game_name).contains("eldenring")));
        // 小幸为外部来源：可打开官网，但不能走 app 内直接下载
        let xiaoxing = reg.search("black myth", &zh, 50);
        assert!(xiaoxing.iter().any(|h| {
            h.source_id == XIAOXING_SOURCE_ID
                && h.external_url.is_some()
                && h.download_url.is_none()
        }));
        // 每个条目至少带一个可用下载地址（用高频词扫到大批条目再抽查）
        let hits = reg.search("the", &zh, 200);
        let no_dl = hits.iter().filter(|h| h.total_downloads == 0).count();
        assert_eq!(no_dl, 0, "无附件条目不应存在");
    }
}
