//! 单流下载：按每次收到的数据重置停滞期限，并在等待网络时响应取消。
use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const CANCEL_POLL: Duration = Duration::from_millis(100);

async fn wait_for_data<T>(
    future: impl Future<Output = Result<T, reqwest::Error>>,
    cancel: &AtomicBool,
    idle: Duration,
) -> Result<T, String> {
    let mut future = std::pin::pin!(future);
    let deadline = Instant::now() + idle;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(super::cancelled_error());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("下载停滞：长时间未收到数据，请重试".to_string());
        }
        match tokio::time::timeout(remaining.min(CANCEL_POLL), future.as_mut()).await {
            Ok(result) => return result.map_err(|e| format!("下载中断: {e}")),
            Err(_) => continue,
        }
    }
}

pub fn transfer(
    url: &reqwest::Url,
    referer: &str,
    file: &Path,
    cancel: &AtomicBool,
    emit: &dyn Fn(&str, u64, u64),
    fallback_total: u64,
) -> Result<u64, String> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0 Safari/537.36")
        .connect_timeout(Duration::from_secs(15));
    if let Some(proxy) = super::effective_proxy() {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|e| e.to_string())?);
    }
    let client = builder.build().map_err(|e| format!("创建下载客户端失败: {e}"))?;
    tauri::async_runtime::block_on(to_file(
        &client, url, referer, file, cancel, emit, fallback_total, IDLE_TIMEOUT,
    ))
}

#[allow(clippy::too_many_arguments)]
async fn to_file(
    client: &reqwest::Client,
    url: &reqwest::Url,
    referer: &str,
    path: &Path,
    cancel: &AtomicBool,
    emit: &dyn Fn(&str, u64, u64),
    fallback_total: u64,
    idle: Duration,
) -> Result<u64, String> {
    let mut response = wait_for_data(
        client.get(url.clone()).header("Referer", referer).send(), cancel, idle,
    ).await?.error_for_status().map_err(|e| format!("下载被拒绝: {e}"))?;
    let declared = response.content_length().unwrap_or(0);
    let total = if declared > 0 { declared } else { fallback_total };
    let mut output = std::fs::File::create(path).map_err(|e| format!("创建临时文件失败: {e}"))?;
    let mut done = 0;
    let mut last_emit = Instant::now();
    while let Some(bytes) = wait_for_data(response.chunk(), cancel, idle).await? {
        if cancel.load(Ordering::SeqCst) {
            return Err(super::cancelled_error());
        }
        output.write_all(&bytes).map_err(|e| format!("写入失败: {e}"))?;
        done += bytes.len() as u64;
        if last_emit.elapsed() >= Duration::from_millis(150) {
            emit("download", done, super::shown_total(total, done));
            last_emit = Instant::now();
        }
    }
    super::ensure_not_cancelled(cancel)?;
    output.flush().map_err(|e| e.to_string())?;
    if declared > 0 && done < declared {
        return Err(format!("下载不完整：应收到 {declared} 字节，实际只收到 {done} 字节"));
    }
    emit("download", done, super::shown_total(total, done));
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    fn server(length: usize, parts: Vec<(Duration, &'static [u8])>) -> (reqwest::Url, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = reqwest::Url::parse(&format!("http://{}/fixture", listener.local_addr().unwrap())).unwrap();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").unwrap();
            for (delay, bytes) in parts {
                std::thread::sleep(delay);
                if stream.write_all(bytes).is_err() { break; }
            }
        });
        (url, thread)
    }

    async fn fetch(url: &reqwest::Url, cancel: &AtomicBool, idle: Duration) -> Result<u64, String> {
        let path = std::env::temp_dir().join(format!("gmc-transfer-{}", uuid::Uuid::new_v4()));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let result = to_file(&client, url, "http://localhost/", &path, cancel, &|_, _, _| {}, 0, idle).await;
        let _ = std::fs::remove_file(path);
        result
    }

    #[test]
    fn stall_deadline_resets_after_each_chunk() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let (url, thread) = server(12, vec![
            (Duration::ZERO, b"abc"),
            (Duration::from_millis(70), b"def"),
            (Duration::from_millis(70), b"ghi"),
            (Duration::from_millis(70), b"jkl"),
        ]);
        assert_eq!(fetch(&url, &AtomicBool::new(false), Duration::from_millis(150)).await.unwrap(), 12);
        thread.join().unwrap();
        });
    }

    #[test]
    fn stalled_body_fails_before_server_finishes() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let (url, thread) = server(6, vec![
            (Duration::ZERO, b"abc"),
            (Duration::from_millis(200), b"def"),
        ]);
        let result = fetch(&url, &AtomicBool::new(false), Duration::from_millis(40)).await;
        assert!(result.unwrap_err().contains("停滞"));
        thread.join().unwrap();
        });
    }

    #[test]
    fn cancel_interrupts_a_pending_network_read() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let (url, thread) = server(6, vec![(Duration::from_millis(400), b"abcdef")]);
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            flag.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        assert_eq!(fetch(&url, &cancel, Duration::from_secs(30)).await.unwrap_err(), super::super::cancelled_error());
        assert!(start.elapsed() < Duration::from_millis(300));
        thread.join().unwrap();
        });
    }

    #[test]
    fn truncated_content_length_is_rejected() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let (url, thread) = server(6, vec![(Duration::ZERO, b"abc")]);
        assert!(fetch(&url, &AtomicBool::new(false), Duration::from_secs(1)).await.is_err());
        thread.join().unwrap();
        });
    }
}
