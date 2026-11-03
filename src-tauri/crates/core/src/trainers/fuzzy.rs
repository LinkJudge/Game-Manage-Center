//! 检索用模糊匹配器 —— 缩写、子序列模糊、数字罗马化、拼音桥接。
//!
//! 行为规格见 SPEC.md §11「检索」：在归一化子串之外，补充参考实现验证过的
//! 三类召回手段（各策略独立命中即算匹配）：
//!   1. 缩写：查询词首字母串与目标名首字母串匹配（gtav → Grand Theft Auto V）
//!   2. 模糊：长查询（≥5 字符）对目标名做局部比率 ≥ 0.8 的编辑距离匹配
//!      （blak myth → Black Myth）
//!   3. 拼音桥接：拉丁查询视为拼音，与已解析中文名的拼音串比对
//!      （heishenhua → 黑神话悟空 → Black Myth: Wukong）
//!
//! 数字在归一化时转为罗马数字，使 "doom 3" 与 "DOOM III" 等价。

use crate::trainers::normalize_name;
use pinyin::ToPinyin;

/// 数字段转罗马数字（仅 ASCII 数字；含 0 的段按 GCM 惯例转 "0"）
pub(crate) fn to_roman(num_str: &str) -> String {
    let n: u64 = num_str.parse().unwrap_or(0);
    if n == 0 {
        return "0".to_string();
    }
    const TABLE: [(u64, &str); 13] = [
        (1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"),
        (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i"),
    ];
    let mut rest = n;
    let mut out = String::new();
    for (value, symbol) in TABLE {
        while rest >= value {
            out.push_str(symbol);
            rest -= value;
        }
    }
    out
}

/// 最长公共子序列长度（字符级 DP）
fn lcs_len(a: &[char], b: &[char]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    for ca in a {
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = if ca == cb {
                prev[j] + 1
            } else {
                prev[j + 1].max(cur[j])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// 局部比率：查询串在目标串内最佳对齐的相似度（0.0~1.0，1.0 为精确包含）。
/// 用 LCS/indel 对齐而非固定窗口编辑距离——允许插入/删除式对齐，
/// "blakmyth" vs "blackmyth"（差一个 c）能得到 0.8+ 的分。
pub fn partial_ratio(query: &str, target: &str) -> f64 {
    let a: Vec<char> = query.chars().collect();
    let b: Vec<char> = target.chars().collect();
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if b.len() <= a.len() {
        let l = lcs_len(&a, &b);
        return 2.0 * l as f64 / (a.len() + b.len()) as f64;
    }
    let mut best = 0.0f64;
    for start in 0..=(b.len() - a.len()) {
        let window: Vec<char> = b[start..start + a.len()].to_vec();
        let l = lcs_len(&a, &window);
        best = best.max(l as f64 / a.len() as f64);
        if best >= 1.0 {
            break;
        }
    }
    best
}

/// 模糊折叠：zh/ch/sh→z/c/s，ng→n（吸收常见拼音发音变体）
fn fuzzy_collapse(s: &str) -> String {
    s.replace("zh", "z")
        .replace("ch", "c")
        .replace("sh", "s")
        .replace("ng", "n")
}

/// 中文名 → 模糊折叠后的拼音连写（非汉字字符跳过）；无汉字返回空串
pub fn pinyin_concat_fuzzy(zh: &str) -> String {
    let mut out = String::new();
    for p in zh.to_pinyin().flatten() {
        out.push_str(p.plain());
    }
    if out.is_empty() {
        String::new()
    } else {
        fuzzy_collapse(&out)
    }
}

/// 目标英文名的首字母缩写（按字母数字词取首字符）
pub fn acronym_of(name: &str) -> String {
    name.split(|c: char| !(c.is_ascii_alphanumeric() || c.is_ascii_digit()))
        .filter(|w| !w.is_empty())
        .filter_map(|w| w.chars().next())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// 一次性预计算的查询匹配器（同一查询对全部条目复用）
pub struct QueryMatcher {
    /// 归一化查询（含数字罗马化）
    normalized: String,
    /// 查询首字母缩写（尾部数字剥离）
    acronym: String,
    /// 归一化长度 ≥5 时启用模糊匹配
    fuzzy: bool,
    /// 拼音查询串（模糊折叠；纯拉丁且 ≥3 字符才启用）
    pinyin_q: String,
}

impl QueryMatcher {
    pub fn new(query: &str) -> Self {
        let normalized = crate::trainers::normalize_name(query);
        let latin: String = normalized.chars().filter(|c| c.is_ascii_lowercase()).collect();
        // 查询侧"缩写"= 整个字母数字串剥尾部数字（GCM 同款语义）：
        // 与目标名的首字母缩写做相等/前缀比较（gta5 → gtav == gtav）
        let acronym = latin.trim_end_matches(|c: char| c.is_ascii_digit()).to_string();
        let pinyin_q = if latin.chars().count() >= 3 {
            fuzzy_collapse(&latin)
        } else {
            String::new()
        };
        QueryMatcher {
            fuzzy: normalized.chars().count() >= 5,
            normalized,
            acronym,
            pinyin_q,
        }
    }

    /// 归一化查询长度（供调用方做最短长度拒绝）
    pub fn normalized_chars(&self) -> usize {
        self.normalized.chars().count()
    }

    /// 归一化后的查询文本（供调用方判断字符类别，如单 CJK 字符放行）
    pub fn normalized(&self) -> &str {
        &self.normalized
    }

    /// 目标（英文名 + 可选中文名）的命中相关度；None = 未命中。
    /// 数值只用于排序（越大越靠前），分档保证：
    /// 全等 > 前缀 > 子串 > 缩写 > 拼音桥接 > 模糊纠错——
    /// 让 "elden" 这类查询把含该词的目标排在仅模糊命中的目标之前。
    pub fn match_score(&self, target_name: &str, target_zh: Option<&str>) -> Option<f64> {
        let tn = crate::trainers::normalize_name(target_name);
        let q = &self.normalized;
        // 1. 归一化文本：英文名、中文名双路
        if tn == *q {
            return Some(100.0);
        }
        if tn.starts_with(q.as_str()) {
            return Some(95.0);
        }
        if tn.contains(q.as_str()) {
            return Some(90.0);
        }
        if let Some(zh) = target_zh {
            let zn = normalize_name(zh);
            if zn == *q {
                return Some(88.0);
            }
            if zn.starts_with(q.as_str()) {
                return Some(86.0);
            }
            if zn.contains(q.as_str()) {
                return Some(84.0);
            }
        }
        // 2. 缩写：查询 == 目标缩写，或目标缩写以查询（≥3 字符）开头
        let ac = acronym_of(target_name);
        let qac = &self.acronym;
        if qac.chars().count() >= 2 {
            if ac == *qac {
                return Some(75.0);
            }
            if qac.chars().count() >= 3 && ac.starts_with(qac.as_str()) {
                return Some(70.0);
            }
        }
        // 3. 拼音桥接：拉丁查询 vs 中文名拼音串
        if !self.pinyin_q.is_empty() {
            if let Some(zh) = target_zh {
                let pc = pinyin_concat_fuzzy(zh);
                if !pc.is_empty() {
                    if pc.contains(&self.pinyin_q) {
                        return Some(65.0);
                    }
                    if self.pinyin_q.chars().count() >= 4 {
                        let r = partial_ratio(&self.pinyin_q, &pc);
                        if r >= 0.85 {
                            return Some(40.0 + (r - 0.85) * 100.0);
                        }
                    }
                }
            }
        }
        // 4. 模糊：长查询局部比率阈值（对英文名）
        if self.fuzzy {
            let r = partial_ratio(q, &tn);
            if r >= 0.8 {
                return Some(50.0 + (r - 0.8) * 50.0);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roman_equivalence_in_normalize() {
        let a = crate::trainers::normalize_name("DOOM 3");
        let b = crate::trainers::normalize_name("DOOM III");
        assert_eq!(a, b, "数字与罗马数字应归一化等价: {a} vs {b}");
        assert_eq!(crate::trainers::normalize_name("F1 2019"), crate::trainers::normalize_name("F1 MMXIX"));
    }

    #[test]
    fn partial_ratio_basics() {
        assert!((partial_ratio("blackmyth", "blackmythwukong") - 1.0).abs() < 1e-9);
        assert!(partial_ratio("blakmyth", "blackmythwukong") >= 0.8);
        assert!(partial_ratio("xyz", "eldenring") < 0.5);
    }

    #[test]
    fn acronym_of_names() {
        assert_eq!(acronym_of("Grand Theft Auto V"), "gtav");
        assert_eq!(acronym_of("ELDEN RING"), "er");
        assert_eq!(acronym_of("DOOM"), "d");
    }

    #[test]
    fn pinyin_bridge_matches_romanized_query() {
        let pc = pinyin_concat_fuzzy("黑神话：悟空");
        assert_eq!(pc, "heisenhuawukon"); // ng→n 折叠
        // 查询侧同折叠后应包含
        let q = fuzzy_collapse("heishenhua");
        assert!(pc.contains(&q));
    }

    #[test]
    fn matcher_hits_all_strategies() {
        let hit = |m: QueryMatcher, name: &str, zh: Option<&str>| m.match_score(name, zh).is_some();
        // 子串
        assert!(hit(QueryMatcher::new("elden"), "ELDEN RING", None));
        // 缩写：gtav → Grand Theft Auto V
        assert!(hit(QueryMatcher::new("gta5"), "Grand Theft Auto V", None));
        assert!(hit(QueryMatcher::new("er"), "ELDEN RING", None));
        // 模糊：错拼
        assert!(hit(QueryMatcher::new("blak myth wukong"), "Black Myth: Wukong", None));
        // 拼音：中文经拼音命中英文名条目
        assert!(hit(QueryMatcher::new("heishenhua"), "Black Myth: Wukong", Some("黑神话：悟空")));
        // 不误配
        assert!(!hit(QueryMatcher::new("heishenhua"), "DOOM", None));
        assert!(!hit(QueryMatcher::new("gta5"), "DOOM", None));
    }

    #[test]
    fn match_score_orders_exact_above_fuzzy() {
        let m = QueryMatcher::new("elden");
        let prefix = m.match_score("Elden Ring", None).unwrap();
        let mid = m.match_score("The Elden Ring Companion", None).unwrap();
        let fuzzy = m.match_score("Bravely Default II", None).unwrap(); // 仅模糊命中（elyde 窗口 0.8）
        // 前缀 > 子串 > 模糊
        assert!(prefix > mid, "{prefix} vs {mid}");
        assert!(mid > fuzzy, "{mid} vs {fuzzy}");
        // 拉丁查询不走中文文本路径（"艾尔登法环" 里没有 elden 子串，拼音比率也不到阈值）
        assert!(m.match_score("Whatever", Some("艾尔登法环")).is_none());
        // "olden" 这类共享 4/5 字符的窗口恰好 0.8：仍算命中（rapidfuzz 同款语义）但沉底
        assert!((m.match_score("Persona 4 Golden", None).unwrap() - fuzzy).abs() < 0.01);
    }
}
