//! 最小 Steam KeyValues（VDF）解析器
//!
//! 覆盖 Steam 实际使用的子集：`"key" "value"`、`"key" { ... }`、`//` 行注释、
//! `\\` `\"` `\n` 转义。libraryfolders.vdf 与 loginusers.vdf 均可解析。
//! 重复键保留（loginusers 可能有多个用户、libraryfolders 用 "0" "1" 编号）。

#[derive(Clone, Debug, PartialEq)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn pairs(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Map(pairs) => pairs,
            Vdf::Str(_) => &[],
        }
    }
}

pub fn parse_vdf(input: &str) -> Result<Vdf, String> {
    let bytes = input.as_bytes();
    let mut pos = 0usize;
    let map = parse_map(input, bytes, &mut pos, 0)?;
    skip_ws(input, &mut pos);
    if pos < input.len() {
        return Err(format!("第 {pos} 字节处有多余内容"));
    }
    Ok(map)
}

fn parse_map(input: &str, bytes: &[u8], pos: &mut usize, depth: usize) -> Result<Vdf, String> {
    if depth > 32 {
        return Err("VDF 嵌套过深".to_string());
    }
    let mut pairs: Vec<(String, Vdf)> = Vec::new();
    loop {
        skip_ws(input, pos);
        if *pos >= bytes.len() {
            // Steam 的文件在末层 map 缺少收尾 } 时按结束处理
            return Ok(Vdf::Map(pairs));
        }
        match bytes[*pos] {
            b'}' => {
                *pos += 1;
                return Ok(Vdf::Map(pairs));
            }
            b'"' => {
                let key = parse_string(input, pos)?;
                skip_ws(input, pos);
                if *pos < bytes.len() && bytes[*pos] == b'{' {
                    *pos += 1;
                    let value = parse_map(input, bytes, pos, depth + 1)?;
                    pairs.push((key, value));
                } else {
                    let value = if *pos < bytes.len() && bytes[*pos] == b'"' {
                        Vdf::Str(parse_string(input, pos)?)
                    } else {
                        Vdf::Str(parse_bare(input, pos))
                    };
                    pairs.push((key, value));
                }
            }
            _ => {
                // 无引号键（非常规但容错）
                let key = parse_bare(input, pos);
                if key.is_empty() {
                    return Err(format!("第 {pos} 字节处无法解析"));
                }
                skip_ws(input, pos);
                let value = if *pos < bytes.len() && bytes[*pos] == b'"' {
                    Vdf::Str(parse_string(input, pos)?)
                } else {
                    Vdf::Str(parse_bare(input, pos))
                };
                pairs.push((key, value));
            }
        }
    }
}

fn skip_ws(input: &str, pos: &mut usize) {
    let bytes = input.as_bytes();
    loop {
        while *pos < bytes.len() && (bytes[*pos] as char).is_ascii_whitespace() {
            *pos += 1;
        }
        // 行注释
        if *pos + 1 < bytes.len() && bytes[*pos] == b'/' && bytes[*pos + 1] == b'/' {
            while *pos < bytes.len() && bytes[*pos] != b'\n' {
                *pos += 1;
            }
            continue;
        }
        break;
    }
}

fn parse_string(input: &str, pos: &mut usize) -> Result<String, String> {
    let bytes = input.as_bytes();
    if *pos >= bytes.len() || bytes[*pos] != b'"' {
        return Err(format!("第 {pos} 字节处应为引号"));
    }
    *pos += 1;
    let mut out = String::new();
    while *pos < bytes.len() {
        match bytes[*pos] {
            b'"' => {
                *pos += 1;
                return Ok(out);
            }
            b'\\' if *pos + 1 < bytes.len() => {
                let next = bytes[*pos + 1];
                match next {
                    b'\\' => out.push('\\'),
                    b'"' => out.push('"'),
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    other => out.push(other as char),
                }
                *pos += 2;
            }
            _ => {
                let ch = input[*pos..].chars().next().unwrap();
                out.push(ch);
                *pos += ch.len_utf8();
            }
        }
    }
    Err("字符串未闭合".to_string())
}

fn parse_bare(input: &str, pos: &mut usize) -> String {
    let bytes = input.as_bytes();
    let start = *pos;
    while *pos < bytes.len()
        && !(bytes[*pos] as char).is_ascii_whitespace()
        && bytes[*pos] != b'{'
        && bytes[*pos] != b'}'
    {
        *pos += 1;
    }
    input[start..*pos].to_string()
}

/// Steam libraryfolders.vdf → 各库的 steamapps/common 根目录
pub fn steam_library_roots(vdf: &Vdf) -> Vec<String> {
    let mut roots = Vec::new();
    if let Some(folders) = vdf.get("libraryfolders") {
        for (_, entry) in folders.pairs() {
            if let Some(path) = entry.get("path").and_then(Vdf::as_str) {
                let unescaped = path.replace("\\\\", "\\");
                if !roots.contains(&unescaped) {
                    roots.push(unescaped);
                }
            }
        }
    }
    roots
}

/// Steam libraryfolders.vdf → (库根目录, 该库下的全部 appid)
pub fn steam_library_apps(vdf: &Vdf) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    if let Some(folders) = vdf.get("libraryfolders") {
        for (_, entry) in folders.pairs() {
            let Some(path) = entry.get("path").and_then(Vdf::as_str) else {
                continue;
            };
            let root = path.replace("\\\\", "\\");
            let apps = entry
                .get("apps")
                .map(|apps| apps.pairs().iter().map(|(id, _)| id.clone()).collect())
                .unwrap_or_default();
            out.push((root, apps));
        }
    }
    out
}

/// loginusers.vdf → AutoLogin == 1 的账号
pub fn steam_autologin_account(vdf: &Vdf) -> Option<(String, String, String, String)> {
    let users = vdf.get("users")?;
    for (steam_id64, user) in users.pairs() {
        let autologin = user.get("AutoLogin").and_then(Vdf::as_str)?;
        if autologin == "1" {
            let account_id = steam_id64
                .parse::<u64>()
                .ok()
                .map(|id| (id & 0xFFFF_FFFF).to_string())?;
            return Some((
                steam_id64.clone(),
                account_id,
                user.get("AccountName").and_then(Vdf::as_str)?.to_string(),
                user.get("PersonaName").and_then(Vdf::as_str)?.to_string(),
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_libraryfolders_with_comments_and_escapes() {
        let sample = r#"
"libraryfolders"
{
	// 注释：自动生成的文件
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps"
		{
			"1091500"		"499132"
		}
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"apps" { }
	}
}
"#;
        let vdf = parse_vdf(sample).unwrap();
        let roots = steam_library_roots(&vdf);
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0], "C:\\Program Files (x86)\\Steam");
        assert_eq!(roots[1], "D:\\SteamLibrary");
    }

    #[test]
    fn parses_loginusers_and_picks_autologin() {
        let sample = r#"
"users"
{
	"76561198000000001"
	{
		"AccountName"		"main_acc"
		"PersonaName"		"Joey"
		"MostRecent"		"0"
		"AutoLogin"		"1"
	}
	"76561198000000002"
	{
		"AccountName"		"second"
		"PersonaName"		"Alt"
		"MostRecent"		"1"
		"AutoLogin"		"0"
	}
}
"#;
        let vdf = parse_vdf(sample).unwrap();
        let (id64, account_id, account_name, persona) = steam_autologin_account(&vdf).unwrap();
        assert_eq!(id64, "76561198000000001");
        // SteamID64 掩码出 32 位账号 ID：低 32 位即账号号段
        // 76561198000000001 mod 2^32 = 39734273
        assert_eq!(account_id, "39734273");
        assert_eq!(account_name, "main_acc");
        assert_eq!(persona, "Joey");
    }
}
