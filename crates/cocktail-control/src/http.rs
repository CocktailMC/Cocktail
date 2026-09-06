//! Outbound HTTP: OS certificate store + Windows system proxy.
//!
//! Browser traffic often works while reqwest fails on Windows because:
//! - rustls + Mozilla roots ignore 杀毒 HTTPS 扫描 / 企业 CA
//! - Clash / 系统代理写在 IE/WinINET，进程环境变量里没有 HTTPS_PROXY

use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast;

use crate::instance::InstanceEvent;

const DEFAULT_UA: &str = "Cocktail-Manager/0.1 (https://github.com/CocktailMC/Cocktail)";

static EVENTS: OnceLock<broadcast::Sender<InstanceEvent>> = OnceLock::new();

pub fn attach_events(tx: broadcast::Sender<InstanceEvent>) {
    let _ = EVENTS.set(tx);
}

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
    ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
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

pub struct Transfer {
    pub id: String,
    pub label: String,
    last: Mutex<(Instant, u64)>,
}

impl Transfer {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            label: label.into(),
            last: Mutex::new((Instant::now() - Duration::from_secs(1), 0)),
        }
    }

    pub fn emit(&self, phase: &str, received: u64, total: Option<u64>) {
        let pct = total
            .filter(|t| *t > 0)
            .map(|t| ((received as f64 / t as f64) * 100.0).clamp(0.0, 100.0) as f32);
        let force = phase != "download";
        let should = force || {
            let Ok(mut g) = self.last.lock() else {
                return;
            };
            let (at, prev) = *g;
            let due = at.elapsed() >= Duration::from_millis(120)
                || received.saturating_sub(prev) >= 256 * 1024
                || total.is_some_and(|t| received >= t);
            if due {
                *g = (Instant::now(), received);
            }
            due
        };
        if !should {
            return;
        }
        let Some(tx) = EVENTS.get() else {
            return;
        };
        let _ = tx.send(InstanceEvent::DownloadProgress {
            id: self.id.clone(),
            label: self.label.clone(),
            phase: phase.to_string(),
            received,
            total,
            pct,
        });
    }

    pub fn finish(&self, received: u64, total: Option<u64>) {
        self.emit("done", received, total.or(Some(received)));
    }
}

pub async fn download_to_path(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    job: &Transfer,
) -> anyhow::Result<u64> {
    job.emit("download", 0, None);
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| explain(e, url))?
        .error_for_status()
        .map_err(|e| explain(e, url))?;
    let total = resp.content_length();
    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = resp.bytes_stream();
    let mut written = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| explain(e, url))?;
        written += chunk.len() as u64;
        file.write_all(&chunk).await?;
        job.emit("download", written, total);
    }
    file.flush().await?;
    job.emit("download", written, total);
    Ok(written)
}

pub async fn download_vec(
    client: &reqwest::Client,
    url: &str,
    job: &Transfer,
    max_bytes: u64,
) -> anyhow::Result<Vec<u8>> {
    job.emit("download", 0, None);
    let resp = client
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
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| explain(e, url))?;
        buf.extend_from_slice(&chunk);
        if buf.len() as u64 > max_bytes {
            anyhow::bail!("下载超过 {} bytes 上限", max_bytes);
        }
        job.emit("download", buf.len() as u64, total);
    }
    let n = buf.len() as u64;
    job.finish(n, total.or(Some(n)));
    Ok(buf)
}
