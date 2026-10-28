//! gm-core — Game Manage Center 核心业务逻辑
//!
//! 占位符/模板语义以 PCGamingWiki 数据库字段格式为准，每条规则配套单元测试。

/// 核心库版本号，随核心逻辑独立演进
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn version() -> &'static str {
    VERSION
}

pub mod cloud;
pub mod backup;
pub mod custom;
pub mod database;
pub mod detect;
pub mod module;
pub mod platform;
pub mod paths;
pub mod resolve;
pub mod restore;
pub mod scanner;
pub mod settings;
pub mod trainers;
pub mod vdf;

/// 从游戏标题生成 Steam/GOG 安装目录名的兜底推断：
/// 去掉 Windows 文件系统非法字符 `\/:*?"<>|` 并 trim。
///
/// 与作者库比对：约 60% 命中，未命中时运行时回退到 wiki 路径，无副作用。
pub fn sanitize_folder(title: &str) -> String {
    title
        .chars()
        .filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect::<String>()
        .trim()
        .to_string()
}

/// Windows 路径统一反斜杠（作者库同款规则；占位符内的分隔符由应用解析器归一化）。
pub fn normalize_win_separators(path: &str) -> String {
    path.replace('/', "\\")
}

/// 注册表路径判定：以 HKEY_ 开头，或以 hkcu/hklm/wow64 占位符开头（大小写不敏感）。
pub fn is_registry_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.starts_with("hkey_")
        || lower.starts_with("{{p|hkcu}}")
        || lower.starts_with("{{p|hklm}}")
        || lower.starts_with("{{p|wow64}}")
}

/// 剥离路径内嵌的注释模板 `{{note|...}}` / `{{nn|...}}` / `{{sic|...}}`。
/// 与 JS 版一致：花括号内容不含嵌套花括号时直接移除，跑两遍处理相邻拼接。
pub fn strip_note_templates(path: &str) -> String {
    let mut current = path.to_string();
    for _ in 0..2 {
        current = strip_once(&current);
    }
    current.trim().to_string()
}

fn strip_once(text: &str) -> String {
    const KEYWORDS: [&str; 3] = ["note", "nn", "sic"];
    let mut out = String::with_capacity(text.len());
    let mut rest = 0usize;

    // 只在原文上做 ASCII 定位（避免 to_lowercase 改变字节长度导致索引错位）
    while let Some(rel) = text[rest..].find("{{") {
        let brace_start = rest + rel;
        match text[brace_start..].find("}}") {
            Some(rel_end) => {
                let end = brace_start + rel_end + 2;
                let inner = &text[brace_start + 2..brace_start + rel_end];
                let is_note = KEYWORDS.iter().any(|kw| {
                    inner
                        .split('|')
                        .next()
                        .map(|name| name.trim().eq_ignore_ascii_case(kw))
                        .unwrap_or(false)
                });
                if is_note {
                    out.push_str(&text[rest..brace_start]);
                } else {
                    out.push_str(&text[rest..end]);
                }
                rest = end;
            }
            None => {
                out.push_str(&text[rest..]);
                rest = text.len();
            }
        }
    }
    out.push_str(&text[rest.min(text.len())..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_folder_strips_illegal_chars() {
        assert_eq!(sanitize_folder("ARK: Survival Ascended"), "ARK Survival Ascended");
        assert_eq!(sanitize_folder("Postal 4: No Regerts"), "Postal 4 No Regerts");
        assert_eq!(sanitize_folder("  .hack//G.U. Last Recode "), ".hackG.U. Last Recode");
    }

    #[test]
    fn normalize_win_separators_works() {
        assert_eq!(
            normalize_win_separators("{{P|localappdata}}/P1348/Saved/SaveGames/"),
            "{{P|localappdata}}\\P1348\\Saved\\SaveGames\\"
        );
    }

    #[test]
    fn registry_detection() {
        assert!(is_registry_path("HKEY_CURRENT_USER\\Software\\X"));
        assert!(is_registry_path("{{p|hkcu}}\\Software\\X"));
        assert!(is_registry_path("{{P|HKLM}}\\SOFTWARE\\Y"));
        assert!(!is_registry_path("{{p|game}}\\saves\\"));
        assert!(!is_registry_path("C:\\Users\\x\\saves"));
    }

    #[test]
    fn strip_note_templates_removes_annotations() {
        // 作者库案例：Afterfall Reconquest —— {{note|32-bit version}} 被剥离
        assert_eq!(
            strip_note_templates("{{p|game}}\\SaveGame.sav {{note|32-bit version}}"),
            "{{p|game}}\\SaveGame.sav"
        );
        assert_eq!(
            strip_note_templates("{{p|game}}\\Saved{{Note|Nexon, Microsoft Store and Steam versions}}"),
            "{{p|game}}\\Saved"
        );
        // 保留非 note 模板
        assert_eq!(
            strip_note_templates("{{p|game}}\\saves\\*.sav"),
            "{{p|game}}\\saves\\*.sav"
        );
        // 相邻拼接两遍清洗
        assert_eq!(strip_note_templates("a{{nn|x}}{{nn|y}}b"), "ab");
    }
}
