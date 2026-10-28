//! WebDAV 客户端（云存档）
//!
//! 只做本项目需要的最小集合：连接测试、确保目录、上传、列举、下载。
//! 用 blocking reqwest —— 命令层是同步 fn，跑在 Tauri 的阻塞线程池里，
//! 不会和 async runtime 冲突。

use reqwest::blocking::Client;
use reqwest::Method;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Config {
    /// 根地址，末尾不带斜杠（调用方负责规整）
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub url: String,
    pub status: u16,
    pub server: Option<String>,
    /// 根目录下已有的条目数（用于快速确认路径对不对）
    pub entries: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    /// 服务器返回的原始 href
    pub href: String,
    /// 相对根目录的路径（已解码、去前导斜杠）
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
}

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:"><d:prop>
<d:resourcetype/><d:getcontentlength/><d:getlastmodified/>
</d:prop></d:propfind>"#;

fn client() -> Result<Client, String> {
    Client::builder()
        // 关键：禁用环境变量代理。
        // 应用从开发终端启动时会继承 http_proxy/https_proxy，而 WebDAV 多为国内服务
        // （坚果云 / 群晖 / 自建 Nextcloud），走代理只会卡到超时——表现为"一直上传、
        // 云端看不到文件"。若要访问境外 WebDAV，请自行反代到国内可达地址。
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(8))
        .user_agent("GameManageCenter-WebDAV/1.0")
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))
}

fn auth(req: reqwest::blocking::RequestBuilder, cfg: &Config) -> reqwest::blocking::RequestBuilder {
    match (&cfg.username, &cfg.password) {
        (Some(u), p) if !u.is_empty() => req.basic_auth(u, p.clone()),
        _ => req,
    }
}

/// 把 href 转成相对根目录的路径（解码百分号、去根前缀、去首尾斜杠）
fn to_rel_path(href: &str, base: &str) -> String {
    let decoded = percent_decode(href);
    let base_path = percent_decode(base);
    let trimmed = decoded.strip_prefix(base_path.as_str()).unwrap_or(&decoded);
    trimmed.trim_matches('/').to_string()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 逐个 `<xxx:tag>content</xxx:tag>` 取出内容。
/// 先按带 `d:` 前缀找，找不到再按裸标签找（兼容不同服务器的命名空间写法）。
fn tags(xml: &str, tag: &str) -> Vec<String> {
    let lower = xml.to_lowercase();
    for (open_pat, end_pat) in [
        (format!("<d:{tag}"), format!("</d:{tag}>")),
        (format!("<{tag}"), format!("</{tag}>")),
    ] {
        let mut out = Vec::new();
        let mut from = 0;
        while let Some(rel) = lower[from..].find(&open_pat) {
            let open = from + rel;
            let Some(gt_rel) = lower[open..].find('>') else { break };
            let gt = open + gt_rel;
            let Some(close_rel) = lower[gt..].find(&end_pat) else { break };
            let close = gt + close_rel;
            out.push(xml[gt + 1..close].to_string());
            from = close + end_pat.len();
        }
        if !out.is_empty() {
            return out;
        }
    }
    Vec::new()
}

fn parse_entries(xml: &str, base: &str) -> Vec<RemoteEntry> {
    // 关键：必须先按 <response> 分块，再在块内各自取 href/size/collection。
    // 若分别提取所有同名标签再按下标对齐，一旦某个 response 缺少 getcontentlength，
    // 后面的 size 就会整体错位（目录拿到了别人的大小、文件拿到 0）。
    let mut out = Vec::new();
    for block in tags(xml, "response") {
        let Some(href) = tags(&block, "href").into_iter().next() else {
            continue;
        };
        let path = to_rel_path(&href, base);
        // 根目录自身（path 为空）跳过
        if path.is_empty() {
            continue;
        }
        let is_dir = !tags(&block, "collection").is_empty() || href.ends_with('/');
        let size = tags(&block, "getcontentlength")
            .first()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let modified = tags(&block, "getlastmodified").into_iter().next();
        out.push(RemoteEntry { href, path, is_dir, size, modified });
    }
    out
}

/// 连接测试：对根目录发 PROPFIND(Depth: 0)，能拿到 2xx 就算通
pub fn test(cfg: &Config) -> Result<Info, String> {
    let c = client()?;
    let url = format!("{}/", cfg.url.trim_end_matches('/'));
    let req = c
        .request(Method::from_bytes(b"PROPFIND").unwrap(), &url)
        .header("Depth", "1")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(PROPFIND_BODY.to_string());
    let resp = auth(req, cfg)
        .send()
        .map_err(|e| format!("连接失败：{e}"))?;
    let status = resp.status();
    let server = resp
        .headers()
        .get("server")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err("认证失败：用户名或密码不正确（401）".into());
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err("路径不存在（404）：请检查地址是否为 WebDAV 根目录".into());
    }
    if !status.is_success() && status.as_u16() != 207 {
        return Err(format!("服务器返回 {}：{status}", status.as_u16()));
    }
    let body = resp.text().unwrap_or_default();
    let entries = parse_entries(&body, &url).len();
    Ok(Info { url, status: status.as_u16(), server, entries })
}

/// 逐级创建目录（已存在则跳过）
pub fn ensure_dir(cfg: &Config, segments: &[&str]) -> Result<(), String> {
    let c = client()?;
    let mut path = cfg.url.trim_end_matches('/').to_string();
    for seg in segments {
        if seg.is_empty() {
            continue;
        }
        path.push('/');
        path.push_str(seg);
        let req = c
            .request(Method::from_bytes(b"MKCOL").unwrap(), &path)
            .header("Content-Length", "0");
        eprintln!("[webdav] MKCOL {path}");
        let resp = auth(req, cfg).send().map_err(|e| format!("创建目录失败：{e}"))?;
        eprintln!("[webdav] -> HTTP {}", resp.status().as_u16());
        let code = resp.status().as_u16();
        // 201=创建成功 405=已存在 301/302=已存在
        if !(resp.status().is_success() || code == 405 || code == 301 || code == 302) {
            return Err(format!("创建目录 {path} 失败：HTTP {code}"));
        }
    }
    Ok(())
}

/// 上传（覆盖同名文件）
pub fn upload(cfg: &Config, remote_path: &str, bytes: Vec<u8>) -> Result<u64, String> {
    let c = client()?;
    let url = format!(
        "{}/{}",
        cfg.url.trim_end_matches('/'),
        remote_path.trim_start_matches('/')
    );
    let len = bytes.len() as u64;
    eprintln!("[webdav] PUT {url} ({} bytes)", len);
    let req = c.put(&url).body(bytes);
    let resp = auth(req, cfg).send().map_err(|e| format!("上传失败：{e}"))?;
    eprintln!("[webdav] -> HTTP {}", resp.status().as_u16());
    if !resp.status().is_success() {
        return Err(format!("上传失败：HTTP {}", resp.status().as_u16()));
    }
    Ok(len)
}

/// 列举目录（Depth: 1）
pub fn list(cfg: &Config, remote_dir: &str) -> Result<Vec<RemoteEntry>, String> {
    let c = client()?;
    let base = format!(
        "{}/{}",
        cfg.url.trim_end_matches('/'),
        remote_dir.trim_matches('/')
    );
    let url = if base.ends_with('/') { base.clone() } else { format!("{base}/") };
    let req = c
        .request(Method::from_bytes(b"PROPFIND").unwrap(), &url)
        .header("Depth", "1")
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(PROPFIND_BODY.to_string());
    let resp = auth(req, cfg).send().map_err(|e| format!("列举失败：{e}"))?;
    if !resp.status().is_success() && resp.status().as_u16() != 207 {
        return Err(format!("列举失败：HTTP {}", resp.status().as_u16()));
    }
    let body = resp.text().unwrap_or_default();
    Ok(parse_entries(&body, &url))
}

/// 下载
pub fn download(cfg: &Config, remote_path: &str) -> Result<Vec<u8>, String> {
    let c = client()?;
    let url = format!(
        "{}/{}",
        cfg.url.trim_end_matches('/'),
        remote_path.trim_start_matches('/')
    );
    let req = c.get(&url);
    let resp = auth(req, cfg).send().map_err(|e| format!("下载失败：{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败：HTTP {}", resp.status().as_u16()));
    }
    let bytes = resp.bytes().map_err(|e| format!("读取响应失败：{e}"))?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decodes_utf8_paths() {
        assert_eq!(percent_decode("/dav/%E5%AD%98%E6%A1%A3/a.zip"), "/dav/存档/a.zip");
        assert_eq!(percent_decode("/dav/plain/a.zip"), "/dav/plain/a.zip");
    }

    #[test]
    fn rel_path_strips_base_and_slashes() {
        assert_eq!(to_rel_path("/dav/GameManageCenter/", "/dav/"), "GameManageCenter");
        assert_eq!(to_rel_path("/dav/GameManageCenter/a.zip", "/dav/"), "GameManageCenter/a.zip");
    }

    #[test]
    fn parses_propfind_entries() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/dav/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/GameManageCenter/</d:href>
    <d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat>
  </d:response>
  <d:response>
    <d:href>/dav/a.zip</d:href>
    <d:propstat><d:prop>
      <d:resourcetype/>
      <d:getcontentlength>1024</d:getcontentlength>
      <d:getlastmodified>Wed, 29 Oct 2026 05:00:00 GMT</d:getlastmodified>
    </d:prop></d:propstat>
  </d:response>
</d:multistatus>"#;
        let entries = parse_entries(xml, "/dav/");
        assert_eq!(entries.len(), 2, "根目录自身应被跳过: {entries:?}");
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].path, "GameManageCenter");
        assert_eq!(entries[1].path, "a.zip");
        assert_eq!(entries[1].size, 1024);
    }
}
