//! HTTP 下载与代理：阶段 2 起 cocktail-init 接管用户空间文件下载。
//!
//! control 端通过 IPC `http.download_to_path` 调用本模块。下载进度推送
//! 留到阶段 2 event push 通道就位后再实现（当前只返最终字节数，
//! control 端 `Transfer::finish` 只发一次"done"事件）。
//!
//! 代理检测策略与 control 端 `http.rs` 完全一致：读 `COCKTAIL_PROXY`
//! 显式代理 → 否则若未设 `HTTPS_PROXY` 等环境变量则查 Windows IE 系统代理。
//! init 子进程默认继承父进程 env，所以 control 设的代理变量会自动透传。

use std::path::Path;
use std::time::Duration;

use anyhow::Context as _;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

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

/// 下载 url 到 dest。返回写入字节数。dest 不存在会自动创建父目录。
///
/// TODO 阶段 2：通过 event push 把 init 端下载进度回推给 control 的
/// `Transfer::emit`，目前 control 端只在完成后拿到字节数发一次 done。
pub async fn download_to_path(url: &str, dest: &Path) -> anyhow::Result<u64> {
    tracing::info!(%url, dest = %dest.display(), "download_to_path: sending request");
    let resp = client()
        .get(url)
        .send()
        .await
        .map_err(|e| explain(e, url))?
        .error_for_status()
        .map_err(|e| explain(e, url))?;
    let total = resp.content_length();
    tracing::info!(%url, status = %resp.status(), content_length = total, dest = %dest.display(), "download_to_path: response received");
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            tracing::debug!(parent = %parent.display(), "download_to_path: ensuring parent dir exists");
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!(
                    "create parent dir {} for {}",
                    parent.display(),
                    dest.display()
                )
            })?;
        }
    }
    tracing::debug!(dest = %dest.display(), "download_to_path: creating file");
    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("create file {}", dest.display()))?;
    let mut stream = resp.bytes_stream();
    let mut written = 0u64;
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
    }
    file.flush()
        .await
        .with_context(|| format!("flush file {}", dest.display()))?;
    tracing::info!(%url, dest = %dest.display(), written, "download_to_path: complete");
    Ok(written)
}

/// 下载 url 到内存 Vec<u8>，超过 max_bytes 上限会提前 bail。
///
/// TODO 阶段 2：通过 event push 把 init 端下载进度回推给 control 的
/// `Transfer::emit`，目前只在 tracing 里记录起止。
pub async fn download_vec(url: &str, max_bytes: u64) -> anyhow::Result<Vec<u8>> {
    tracing::info!(%url, "download_vec: sending request");
    let resp = client()
        .get(url)
        .send()
        .await
        .map_err(|e| explain(e, url))?
        .error_for_status()
        .map_err(|e| explain(e, url))?;
    let total = resp.content_length();
    tracing::info!(
        %url,
        status = %resp.status(),
        content_length = total,
        "download_vec: response received"
    );
    let mut stream = resp.bytes_stream();
    let mut buf = Vec::new();
    if let Some(t) = total.filter(|n| *n > 0 && *n <= max_bytes) {
        buf.reserve(t as usize);
    }
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| explain(e, url))?;
        buf.extend_from_slice(&chunk);
        if buf.len() as u64 > max_bytes {
            anyhow::bail!("下载超过 {} bytes 上限", max_bytes);
        }
        // TODO 阶段 2：通过 event push 把 init 端下载进度回推给 control 的 Transfer::emit。
    }
    let n = buf.len() as u64;
    tracing::info!(%url, bytes = n, "download_vec: complete");
    Ok(buf)
}
