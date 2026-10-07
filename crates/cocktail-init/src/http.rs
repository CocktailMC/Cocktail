//! HTTP 下载与代理：阶段 2 起 cocktail-init 接管用户空间文件下载。
//!
//! control 端通过 IPC `http.download_to_path` 调用本模块。下载进度以统一格式的
//! 实时行（`download.running`）输出到 stderr，由 control 端采集；同时通过
//! [`crate::events::emit`] 回推 `download.started/progress/completed/failed`
//! 事件给 control（跨进程 event push 通道）。
//!
//! 代理检测策略与 control 端 `http.rs` 完全一致：读 `COCKTAIL_PROXY`
//! 显式代理 → 否则若未设 `HTTPS_PROXY` 等环境变量则查 Windows IE 系统代理。
//! init 子进程默认继承父进程 env，所以 control 设的代理变量会自动透传。

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use cocktail_shared::logfmt::{human_bytes, human_duration};
use cocktail_shared::logging::LiveLine;

use crate::events;

const DEFAULT_UA: &str = "Cocktail-Manager/0.1 (https://github.com/CocktailMC/Cocktail)";

pub fn builder() -> reqwest::ClientBuilder {
    let mut b = reqwest::Client::builder()
        .user_agent(DEFAULT_UA)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .pool_idle_timeout(Duration::from_secs(30))
        .tcp_nodelay(true)
        .redirect(reqwest::redirect::Policy::limited(16));

    #[cfg(windows)]
    {
        b = b.use_native_tls();
        b = b.local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    }

    if let Some(proxy) = explicit_proxy() {
        b = b.proxy(proxy);
    } else if !env_proxy_set() {
        if let Some((proxy, url)) = windows_system_proxy() {
            tracing::info!(%url, "using Windows system proxy");
            b = b.proxy(proxy);
        }
    }
    b
}

pub fn client() -> reqwest::Client {
    builder().build().unwrap_or_else(|e| {
        tracing::warn!(error = %e, "http client build failed; using defaults");
        reqwest::Client::new()
    })
}

pub fn explain(err: reqwest::Error, url: &str) -> anyhow::Error {
    let raw = format!("{err:#}");
    let hint = if err.is_timeout() {
        "连接超时。若浏览器能打开该地址，请打开 Clash / 系统代理，或设置环境变量 HTTPS_PROXY。"
    } else if err.is_connect() {
        "无法建立 TCP 连接。检查网络、防火墙，以及系统代理是否开启。"
    } else if looks_like_tls(&raw) {
        "TLS 握手失败。常见于杀毒软件 HTTPS 扫描或未走系统代理；Cocktail 已改用 Windows 系统证书库。"
    } else if looks_like_dns(&raw) {
        "DNS 解析失败。可尝试更换 DNS，或开启系统代理。"
    } else {
        "外网请求失败。Paper / Modrinth / Hangar / Adoptium 都需要能访问国际网络。"
    };
    anyhow::anyhow!("请求 {url} 失败：{hint}（{err}）")
}

fn looks_like_tls(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("certificate")
        || l.contains("tls")
        || l.contains("ssl")
        || l.contains("handshake")
        || l.contains("cert")
}

fn looks_like_dns(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("dns") || l.contains("name or service") || l.contains("resolve")
}

fn env_proxy_set() -> bool {
    [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ]
    .iter()
    .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
}

fn explicit_proxy() -> Option<reqwest::Proxy> {
    let raw = std::env::var("COCKTAIL_PROXY").ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    reqwest::Proxy::all(raw).ok()
}

fn windows_system_proxy() -> Option<(reqwest::Proxy, String)> {
    #[cfg(windows)]
    {
        return winhttp_ie_proxy();
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn winhttp_ie_proxy() -> Option<(reqwest::Proxy, String)> {
    #[repr(C)]
    struct IeProxy {
        auto_detect: i32,
        auto_config_url: *mut u16,
        proxy: *mut u16,
        proxy_bypass: *mut u16,
    }

    #[link(name = "winhttp")]
    unsafe extern "system" {
        fn WinHttpGetIEProxyConfigForCurrentUser(cfg: *mut IeProxy) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalFree(h: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }

    unsafe fn take_wide(p: *mut u16) -> Option<String> {
        if p.is_null() {
            return None;
        }
        let mut len = 0usize;
        while unsafe { *p.add(len) } != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) });
        unsafe { GlobalFree(p.cast()) };
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    }

    let mut cfg = IeProxy {
        auto_detect: 0,
        auto_config_url: std::ptr::null_mut(),
        proxy: std::ptr::null_mut(),
        proxy_bypass: std::ptr::null_mut(),
    };
    let ok = unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut cfg) };
    if ok == 0 {
        unsafe {
            let _ = take_wide(cfg.auto_config_url);
            let _ = take_wide(cfg.proxy);
            let _ = take_wide(cfg.proxy_bypass);
        }
        return None;
    }
    let auto_url = unsafe { take_wide(cfg.auto_config_url) };
    let proxy = unsafe { take_wide(cfg.proxy) };
    let bypass = unsafe { take_wide(cfg.proxy_bypass) };
    if let Some(url) = auto_url {
        tracing::debug!(%url, "Windows PAC / auto-proxy URL is set; set HTTPS_PROXY if downloads still fail");
    }
    let server = proxy?;
    let url = pick_proxy_url(&server)?;
    let mut p = reqwest::Proxy::all(&url).ok()?;
    let bypass = bypass.unwrap_or_default();
    let mut no = bypass.replace(';', ",");
    if !no.to_ascii_lowercase().contains("localhost") {
        no.push_str(",localhost,127.0.0.1,::1");
    }
    p = p.no_proxy(reqwest::NoProxy::from_string(&no));
    Some((p, url))
}

fn pick_proxy_url(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let lower = raw.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("socks5://")
        || lower.starts_with("socks://")
    {
        return Some(raw.to_string());
    }
    let mut http = None;
    let mut socks = None;
    for part in raw.split([';', ' ', '\t']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((k, v)) = part.split_once('=') {
            let k = k.trim().to_ascii_lowercase();
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            if k == "https" || k == "http" {
                http = Some(v.to_string());
            } else if k == "socks" || k == "socks5" {
                socks = Some(v.to_string());
            }
        } else if http.is_none() {
            http = Some(part.to_string());
        }
    }
    if let Some(h) = http {
        return Some(with_scheme(&h, "http"));
    }
    socks.map(|s| with_scheme(&s, "socks5"))
}

fn with_scheme(addr: &str, scheme: &str) -> String {
    if addr.contains("://") {
        addr.to_string()
    } else {
        format!("{scheme}://{addr}")
    }
}

/// 下载进度 kv：`received` 必有，`total` 未知时省略。
fn progress_kv(written: u64, total: Option<u64>) -> Vec<(String, String)> {
    let mut kv = vec![("received".to_string(), human_bytes(written))];
    if let Some(t) = total {
        kv.push(("total".to_string(), human_bytes(t)));
    }
    kv
}

/// 下载 url 到 dest。返回写入字节数。dest 不存在会自动创建父目录。
///
/// 进度以 `[ **** ][cocktail-http] download.running ...` 实时行输出到 stderr，
/// 完成/失败分别收尾为 `download.completed` / `download.failed`。
pub async fn download_to_path(url: &str, dest: &Path) -> anyhow::Result<u64> {
    let dest_label = dest
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| dest.display().to_string());
    let started = Instant::now();
    // 本地 stderr 实时行（给人看）与跨进程 event（回推 control/UI）并存。
    events::emit(
        "download.started",
        serde_json::json!({ "url": url, "dest": dest_label.clone() }),
    );
    let live = LiveLine::begin(
        "cocktail-http",
        "download.running",
        vec![("dest".to_string(), dest_label.clone())],
    );
    match download_to_path_inner(url, dest, &live).await {
        Ok(written) => {
            events::emit(
                "download.completed",
                serde_json::json!({
                    "url": url,
                    "dest": dest_label,
                    "bytes": written,
                    "duration_ms": started.elapsed().as_millis() as u64,
                }),
            );
            live.done(
                "download.completed",
                vec![
                    ("bytes".to_string(), human_bytes(written)),
                    ("duration".to_string(), human_duration(started.elapsed())),
                ],
            );
            Ok(written)
        }
        Err(e) => {
            events::emit(
                "download.failed",
                serde_json::json!({
                    "url": url,
                    "dest": dest_label,
                    "error": format!("{e:#}"),
                }),
            );
            live.fail("download.failed", &format!("{e:#}"));
            Err(e)
        }
    }
}

/// `download_to_path` 的实际实现；进度通过 `live` 实时刷新。
async fn download_to_path_inner(url: &str, dest: &Path, live: &LiveLine) -> anyhow::Result<u64> {
    let resp = client()
        .get(url)
        .send()
        .await
        .map_err(|e| explain(e, url))?
        .error_for_status()
        .map_err(|e| explain(e, url))?;
    let total = resp.content_length();
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!(
                    "create parent dir {} for {}",
                    parent.display(),
                    dest.display()
                )
            })?;
        }
    }
    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("create file {}", dest.display()))?;
    let mut stream = resp.bytes_stream();
    let mut written = 0u64;
    let mut last = Instant::now();
    let mut last_bytes = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| explain(e, url))?;
        written += chunk.len() as u64;
        file.write_all(&chunk).await.with_context(|| {
            format!(
                "write to file {} ({} bytes written)",
                dest.display(),
                written
            )
        })?;
        // 节流：≥200ms 或自上次刷新后新增 ≥256KiB 才更新实时行 + emit 一次
        // progress，避免小 chunk 下载造成事件风暴。
        if last.elapsed() >= Duration::from_millis(200)
            || written.saturating_sub(last_bytes) >= 256 * 1024
        {
            live.update(progress_kv(written, total));
            events::emit(
                "download.progress",
                serde_json::json!({ "url": url, "received": written, "total": total }),
            );
            last = Instant::now();
            last_bytes = written;
        }
    }
    file.flush()
        .await
        .with_context(|| format!("flush file {}", dest.display()))?;
    Ok(written)
}

/// 下载 url 到内存 Vec<u8>，超过 max_bytes 上限会提前 bail。
///
/// 进度同上，以 `download.running` 实时行输出到 stderr。
pub async fn download_vec(url: &str, max_bytes: u64) -> anyhow::Result<Vec<u8>> {
    let started = Instant::now();
    let live = LiveLine::begin(
        "cocktail-http",
        "download.running",
        vec![("url".to_string(), url.to_string())],
    );
    match download_vec_inner(url, max_bytes, &live).await {
        Ok(buf) => {
            live.done(
                "download.completed",
                vec![
                    ("bytes".to_string(), human_bytes(buf.len() as u64)),
                    ("duration".to_string(), human_duration(started.elapsed())),
                ],
            );
            Ok(buf)
        }
        Err(e) => {
            live.fail("download.failed", &format!("{e:#}"));
            Err(e)
        }
    }
}

async fn download_vec_inner(url: &str, max_bytes: u64, live: &LiveLine) -> anyhow::Result<Vec<u8>> {
    let resp = client()
        .get(url)
        .send()
        .await
        .map_err(|e| explain(e, url))?
        .error_for_status()
        .map_err(|e| explain(e, url))?;
    let total = resp.content_length();
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::new();
    if let Some(t) = total.filter(|n| *n > 0 && *n <= max_bytes) {
        buf.reserve(t as usize);
    }
    let mut last = Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| explain(e, url))?;
        buf.extend_from_slice(&chunk);
        if buf.len() as u64 > max_bytes {
            anyhow::bail!("下载超过 {} bytes 上限", max_bytes);
        }
        if last.elapsed() >= Duration::from_millis(200) {
            live.update(progress_kv(buf.len() as u64, total));
            last = Instant::now();
        }
    }
    Ok(buf)
}
