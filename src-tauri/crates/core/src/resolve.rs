//! 模板解析器（行为规格见 SPEC.md §2/§3/§4）
//!
//! 架构：分段词法流水线。
//! 1. `lex`    模板 → 段序列（段间以 \\ 或 / 分隔），段内是 Piece 序列
//!    （字面 / 路径变量 / 上下文变量 / 注册表根键 / uid 槽 / 通配）
//! 2. `expand` 变量 → 具体值；uid 槽按账号组合替换；通配保留 `*`
//! 3. `match`  具体模式在文件系统上匹配（`*` 只匹配单目录段）
//! 4. 兜底     无组合命中时 uid 全部置 `*`，取最近修改的单个结果
//! 5. `display` 匹配路径上的已知变量值回填为 {{p|...}} 字样，作为清单模板
//!
//! 与"整串正则替换"的实现路线不同：值替换在段对齐的基础上进行，
//! 通配段的行为由匹配结果决定。

use crate::paths;
use crate::platform::Platform;
use serde::Serialize;
use std::path::Path;

/// 全部已知账号 ID
#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountIds {
    pub steam: Option<String>,
    pub ubisoft: Option<String>,
    pub epic: Option<String>,
    pub xbox: Option<String>,
    pub rockstar: Option<String>,
    pub gog: Option<String>,
    pub ea: Option<String>,
}

impl AccountIds {
    /// 非空账号列表（供组合枚举）
    pub fn uid_values(&self) -> Vec<String> {
        [
            &self.steam,
            &self.ubisoft,
            &self.epic,
            &self.xbox,
            &self.rockstar,
            &self.gog,
            &self.ea,
        ]
        .into_iter()
        .flatten()
        .filter(|v| !v.is_empty() && v.as_str() != "N/A")
        .cloned()
        .collect()
    }
}

pub struct ResolveContext<'a> {
    pub platform: Platform,
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub home: &'a Path,
    pub accounts: &'a AccountIds,
    pub game_dir: Option<String>,
    pub steam_dir: Option<String>,
    pub ubisoft_dir: Option<String>,
    /// 兼容保留（当前全局模型不使用）
    pub all_accounts: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ResolvedKind {
    File,
    Registry,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPath {
    /// 源模板
    pub template: String,
    /// 展示模板（占位符回填、通配保留）
    pub final_template: String,
    /// 具体路径（正斜杠）
    pub resolved: String,
    pub kind: ResolvedKind,
    /// saves | config | reg
    pub content_kind: String,
}

// ======================================================================
// 词法
// ======================================================================

#[derive(Clone, Debug)]
enum Piece {
    Text(String),
    /// 路径变量（规格 §1 表内的普通变量）
    Var(String),
    /// 上下文变量：game / steam / ubisoft
    Ctx(&'static str),
    /// 注册表根键：hkcu / hklm / wow64
    Reg(String),
    /// Steam 账号 ID 槽位
    Uid,
    /// 通配（`*`；`?` 按字面处理——数据库模板只用 `*`）
    Star,
}

fn lex_segments(template: &str) -> Vec<Vec<Piece>> {
    let b = template.as_bytes();
    let mut segs: Vec<Vec<Piece>> = vec![Vec::new()];
    let mut text = String::new();
    let mut i = 0usize;

    while i < b.len() {
        // {{p|xxx}} 路径变量——库数据同时存在 {{p| 与 {{P|（以及 {{path| 变体），
        // 标记部分不区分大小写（wiki 数据 ~1/3 条目用大写形态）
        if b[i] == b'{' && i + 1 < b.len() && b[i + 1] == b'{' {
            let head = &template[i + 2..];
            if let Some(pipe) = head.find('|') {
                let marker = head[..pipe].to_ascii_lowercase();
                if marker == "p" || marker == "path" {
                    let content_start = i + 2 + pipe + 1;
                    if let Some(end) = template[content_start..].find("}}") {
                        let raw = &template[content_start..content_start + end];
                        let name = paths::normalize(raw);
                        if !text.is_empty() {
                            segs.last_mut().unwrap().push(Piece::Text(std::mem::take(&mut text)));
                        }
                        segs.last_mut().unwrap().push(match name.as_str() {
                            "uid" => Piece::Uid,
                            "game" => Piece::Ctx("game"),
                            "steam" => Piece::Ctx("steam"),
                            "uplay" | "ubisoftconnect" => Piece::Ctx("ubisoft"),
                            "hkcu" => Piece::Reg("hkcu".into()),
                            "hklm" => Piece::Reg("hklm".into()),
                            "wow64" => Piece::Reg("wow64".into()),
                            other => Piece::Var(other.to_string()),
                        });
                        i = content_start + end + 2;
                        continue;
                    }
                }
            }
        }
        // 通配
        if b[i] == b'*' {
            if !text.is_empty() {
                segs.last_mut().unwrap().push(Piece::Text(std::mem::take(&mut text)));
            }
            segs.last_mut().unwrap().push(Piece::Star);
            i += 1;
            continue;
        }
        // 目录分隔符
        if b[i] == b'\\' || b[i] == b'/' {
            if !text.is_empty() {
                segs.last_mut().unwrap().push(Piece::Text(std::mem::take(&mut text)));
            }
            if segs.last().map(|s| !s.is_empty()).unwrap_or(false) {
                segs.push(Vec::new());
            }
            i += 1;
            continue;
        }
        let ch = template[i..].chars().next().unwrap();
        text.push(ch);
        i += ch.len_utf8();
    }
    if !text.is_empty() {
        segs.last_mut().unwrap().push(Piece::Text(text));
    }
    segs
}

// ======================================================================
// 求值
// ======================================================================

struct Env<'a> {
    ctx: &'a ResolveContext<'a>,
}

impl<'a> Env<'a> {
    fn piece(&self, p: &Piece) -> Option<String> {
        match p {
            Piece::Text(t) => Some(t.clone()),
            Piece::Star => Some("*".to_string()),
            Piece::Uid => None,
            Piece::Ctx(k) => {
                let v = match *k {
                    "game" => self.ctx.game_dir.clone().unwrap_or_default(),
                    "steam" => self.ctx.steam_dir.clone().unwrap_or_default(),
                    "ubisoft" => self.ctx.ubisoft_dir.clone().unwrap_or_default(),
                    _ => String::new(),
                };
                if v.is_empty() { None } else { Some(v) }
            }
            Piece::Reg(n) => match n.as_str() {
                "hkcu" => Some("HKEY_CURRENT_USER".to_string()),
                "hklm" => Some("HKEY_LOCAL_MACHINE".to_string()),
                "wow64" => Some("HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node".to_string()),
                _ => None,
            },
            Piece::Var(n) => {
                crate::paths::resolve_one(n, self.ctx.platform, self.ctx.env, self.ctx.home)
            }
        }
    }
}

// ======================================================================
// 公开 API
// ======================================================================

/// 解析一条文件模板（SPEC §2 五阶段）
pub fn resolve_template(template: &str, ctx: &ResolveContext) -> Vec<ResolvedPath> {
    let segs = lex_segments(template);
    let uid_slots = count_uid(&segs);
    let values = ctx.accounts.uid_values();

    // 组合枚举：uid 槽位 × 账号列表
    for combo in combinations(uid_slots, &values) {
        let Some(pattern) = build_pattern(&segs, ctx, Some(&combo)) else {
            return Vec::new();
        };
        let hits = glob_hits(&pattern);
        if !hits.is_empty() {
            return hits
                .into_iter()
                .map(|path| ResolvedPath {
                    template: template.to_string(),
                    final_template: rebuild_display(&segs, Some(&combo)),
                    resolved: path.clone(),
                    kind: ResolvedKind::File,
                    content_kind: String::new(),
                })
                .collect();
        }
    }

    // 兜底：uid 全部置 `*`，取最近修改的单个结果
    if uid_slots > 0 {
        let wildcard = match build_pattern(&segs, ctx, None) {
            Some(p) => p,
            None => return Vec::new(),
        };
        let hits = glob_hits(&wildcard);
        if let Some(best) = latest(hits) {
            let display = rebuild_display(&segs, None);
            return vec![ResolvedPath {
                template: template.to_string(),
                final_template: display,
                resolved: best,
                kind: ResolvedKind::File,
                content_kind: String::new(),
            }];
        }
    }
    Vec::new()
}

/// 解析一条注册表模板（候选生成；存在性校验由 scanner 完成）
pub fn resolve_registry_template(template: &str, ctx: &ResolveContext) -> Vec<ResolvedPath> {
    let mut base = template.to_string();
    // 收尾分隔符会隐藏键名
    while base.ends_with('\\') || base.ends_with('/') {
        base.pop();
    }
    let segs = lex_segments(&base);

    let mut concrete_segs: Vec<String> = Vec::new();
    let mut display_segs: Vec<String> = Vec::new();
    let mut ok = true;
    for seg in &segs {
        let mut c = String::new();
        let mut d = String::new();
        for piece in seg {
            let v = match piece {
                Piece::Uid => {
                    // 注册表分支：候选里保留组合替换，无组合则放弃
                    ok = false;
                    None
                }
                Piece::Ctx(k) => match *k {
                    "game" => ctx.game_dir.clone(),
                    "steam" => ctx.steam_dir.clone(),
                    "ubisoft" => ctx.ubisoft_dir.clone(),
                    _ => None,
                }
                .filter(|v| !v.is_empty()),
                Piece::Text(t) => Some(t.clone()),
                Piece::Reg(n) => match n.as_str() {
                    "hkcu" => Some("HKEY_CURRENT_USER".to_string()),
                    "hklm" => Some("HKEY_LOCAL_MACHINE".to_string()),
                    "wow64" => Some("HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node".to_string()),
                    _ => None,
                },
                Piece::Var(name) => {
                    crate::paths::resolve_one(name, ctx.platform, &|k: &str| std::env::var(k).ok(), ctx.home)
                }
                _ => None,
            };
            match v {
                Some(v) => {
                    c.push_str(&v);
                    push_display(&mut d, piece);
                }
                None => ok = false,
            }
        }
        if c.is_empty() {
            ok = false;
            break;
        }
        concrete_segs.push(c);
        display_segs.push(d);
    }
    if !ok {
        return Vec::new();
    }
    // 分隔符在词法阶段转为段边界，这里重新接回
    let concrete = concrete_segs.join("\\");
    let display = display_segs.join("\\");
    if concrete.is_empty() {
        return Vec::new();
    }
    vec![ResolvedPath {
        template: template.to_string(),
        final_template: display,
        resolved: concrete,
        kind: ResolvedKind::Registry,
        content_kind: "reg".into(),
    }]
}

// ======================================================================
// 内部：模式构建 / glob / 展示重建
// ======================================================================

fn build_pattern(
    segs: &[Vec<Piece>],
    ctx: &ResolveContext,
    combo: Option<&Vec<String>>,
) -> Option<String> {
    let ev = Env { ctx };
    let mut out: Vec<String> = Vec::new();
    let mut uid_idx = 0usize;
    for seg in segs {
        let mut seg_str = String::new();
        for piece in seg {
            let v = match piece {
                Piece::Uid => {
                    if let Some(combo) = combo {
                        let v = combo.get(uid_idx)?;
                        uid_idx += 1;
                        v.clone()
                    } else {
                        "*".to_string()
                    }
                }
                _ => ev.piece(piece)?,
            };
            seg_str.push_str(&v);
        }
        if !seg_str.is_empty() {
            out.push(seg_str);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("\\"))
    }
}

fn glob_hits(pattern: &str) -> Vec<String> {
    let options = glob::MatchOptions {
        case_sensitive: false,
        require_literal_separator: true, // `*` 只匹配单目录段
        require_literal_leading_dot: false,
    };
    match glob::glob_with(pattern, options) {
        Ok(paths) => paths
            .flatten()
            .map(|p| p.to_string_lossy().to_string())
            .filter(|p| Path::new(p).exists())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn latest(paths: Vec<String>) -> Option<String> {
    let mut best = String::new();
    let mut best_t = std::time::SystemTime::UNIX_EPOCH;
    for p in paths {
        let t = std::fs::metadata(&p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        if t > best_t {
            best_t = t;
            best = p;
        }
    }
    if best.is_empty() { None } else { Some(best) }
}

/// 展示模板：按词法片段重建——变量/上下文/注册表段还原为 {{p|...}} 字样，
/// 字面文本与通配 `*` 原样保留，uid 段保留组合出的具体值（兜底时为 `*`）
fn rebuild_display(segs: &[Vec<Piece>], combo: Option<&Vec<String>>) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut uid_idx = 0usize;
    for seg in segs {
        let mut seg_str = String::new();
        for piece in seg {
            match piece {
                Piece::Text(t) => seg_str.push_str(t),
                Piece::Star => seg_str.push('*'),
                Piece::Ctx(k) => seg_str.push_str(&format!("{{{{p|{k}}}}}")),
                Piece::Reg(n) | Piece::Var(n) => {
                    seg_str.push_str(&format!("{{{{p|{n}}}}}"));
                }
                Piece::Uid => {
                    if let Some(v) = combo.and_then(|c| c.get(uid_idx)) {
                        uid_idx += 1;
                        seg_str.push_str(v);
                    } else {
                        seg_str.push('*');
                    }
                }
            }
        }
        if !seg_str.is_empty() {
            out.push(seg_str);
        }
    }
    out.join("\\")
}

fn push_display(display: &mut String, piece: &Piece) {
    match piece {
        Piece::Text(t) => display.push_str(t),
        Piece::Star => display.push('*'),
        Piece::Ctx(k) => display.push_str(&format!("{{{{p|{k}}}}}")),
        Piece::Reg(n) => display.push_str(&format!("{{{{p|{n}}}}}")),
        Piece::Var(n) => display.push_str(&format!("{{{{p|{n}}}}}")),
        // 注册表分支不消费 uid（外层已置 ok=false）
        Piece::Uid => {}
    }
}

fn count_uid(segs: &[Vec<Piece>]) -> usize {
    segs.iter()
        .flat_map(|s| s.iter())
        .filter(|p| matches!(p, Piece::Uid))
        .count()
}

fn combinations(count: usize, values: &[String]) -> Vec<Vec<String>> {
    let mut result: Vec<Vec<String>> = vec![Vec::new()];
    for _ in 0..count {
        let mut next = Vec::with_capacity(result.len() * values.len().max(1));
        for combo in &result {
            for v in values {
                let mut c = combo.clone();
                c.push(v.clone());
                next.push(c);
            }
        }
        result = next;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_ctx() -> ResolveContext<'static> {
        // 泄漏闭包/主页/账号，测试生命周期内可用
        let env: &'static dyn Fn(&str) -> Option<String> = &|_: &str| None;
        let home: &'static Path = Box::leak(Box::new(std::env::temp_dir()));
        ResolveContext {
            platform: Platform::Windows,
            env,
            home,
            game_dir: None,
            steam_dir: None,
            ubisoft_dir: None,
            accounts: Box::leak(Box::new(AccountIds::default())),
            all_accounts: false,
        }
    }

    #[test]
    fn registry_template_joins_segments_and_display() {
        let ctx = base_ctx();
        let out = resolve_registry_template(r"{{p|hkcu}}\Software\DemoGame", &ctx);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].resolved, r"HKEY_CURRENT_USER\Software\DemoGame");
        assert_eq!(out[0].final_template, r"{{p|hkcu}}\Software\DemoGame");
        assert_eq!(out[0].kind, ResolvedKind::Registry);
    }

    #[test]
    fn registry_template_strips_trailing_separator() {
        let ctx = base_ctx();
        let out = resolve_registry_template(r"{{p|wow64}}\Vendor\Game\", &ctx);
        assert_eq!(out[0].resolved, r"HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Vendor\Game");
    }

    #[test]
    fn registry_template_missing_var_fails() {
        let ctx = base_ctx();
        assert!(resolve_registry_template(r"{{p|hkcu}}\Software\{{p|uid}}\Game", &ctx).is_empty());
    }

    #[test]
    fn file_template_display_restores_variables() {
        let dir = std::env::temp_dir().join(format!("gm-resolve-{}", std::process::id()));
        let saves = dir.join("Demo").join("saves");
        std::fs::create_dir_all(&saves).unwrap();
        std::fs::write(saves.join("a.sav"), b"x").unwrap();

        let ctx = ResolveContext {
            game_dir: Some(dir.join("Demo").to_string_lossy().to_string()),
            ..base_ctx()
        };
        let out = resolve_template(r"{{p|game}}\saves\*.sav", &ctx);
        assert_eq!(out.len(), 1);
        assert!(out[0].final_template.contains("{{p|game}}"));
        assert!(out[0].final_template.ends_with(r"\saves\*.sav"));
        assert!(out[0].resolved.ends_with("a.sav"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lex_keeps_final_character_of_template() {
        // 回归：曾因 i+1<len 提前退出丢弃末字符（*.sav → *.sa）
        let segs = lex_segments(r"C:\Save\*.sav");
        let last = segs.last().unwrap().last().unwrap();
        match last {
            Piece::Text(t) => assert_eq!(t, ".sav"),
            other => panic!("unexpected piece: {other:?}"),
        }
    }

    #[test]
    fn placeholder_marker_is_case_insensitive() {
        // 回归：库数据 ~1/3 条目用大写 {{P|（GTA V / RDR2 / 只狼等），
        // 词法器曾只认小写导致这批游戏存档全部无法解析
        let dir = std::env::temp_dir().join(format!("gm-case-{}", std::process::id()));
        let game_dir = dir.join("Sekiro");
        std::fs::create_dir_all(&game_dir).unwrap();
        std::fs::write(game_dir.join("GraphicsConfig.xml"), b"x").unwrap();

        let ctx = ResolveContext {
            game_dir: Some(game_dir.to_string_lossy().to_string()),
            ..base_ctx()
        };
        // {{P|...}} 大写形态
        let out = resolve_template(r"{{P|game}}\GraphicsConfig.xml", &ctx);
        assert_eq!(out.len(), 1, "大写 {{P| 必须被识别");
        // {{Path|...}} 变体
        let out = resolve_template(r"{{Path|game}}\GraphicsConfig.xml", &ctx);
        assert_eq!(out.len(), 1, "{{Path| 变体必须被识别");
        // 同一模板内大小写混用（真实数据形态）：词法层断言两个占位符都被识别
        let segs = lex_segments(r"{{p|game}}\{{P|uid}}\save.dat");
        let flat: Vec<&Piece> = segs.iter().flatten().collect();
        assert!(
            flat.iter().any(|p| matches!(p, Piece::Ctx("game"))),
            "小写 {{p| 必须识别"
        );
        assert!(
            flat.iter().any(|p| matches!(p, Piece::Uid)),
            "大写 {{P|uid}} 必须识别"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
