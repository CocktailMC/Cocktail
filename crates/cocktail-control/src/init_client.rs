//! cocktail-init 子进程 IPC client。
//!
//! control 启动时调 `InitSupervisor::spawn_initial()` fork+exec cocktail-init
//! 子进程，通过 stdin/stdout pipe + 长度前缀 JSON-RPC 2.0 双向通信。
//!
//! 生命周期（阶段 2 起）：
//! - `InitSupervisor` 持有可更新句柄 `RwLock<Option<Arc<InitClient>>>`，
//!   init 崩溃（reader_loop EOF）后由 supervisor respawn，重建握手；
//! - respawn 限流：30s 冷却，防止 init 反复崩溃导致 respawn 风暴；
//! - `shutdown()` 主动关闭 stdin pipe，init 端 reader EOF 自然退出。
//!
//! fallback 设计：spawn 或 call 失败时返回 Err，调用方退化到本地函数
//! （如 secrets::load_or_create 直接读文件），保证 init 不可达时
//! control 仍能鉴权与运行（兼容单进程老部署）。
//!
//! TODO 阶段 3：watchdog 周期 ping init，超时触发 respawn（当前只在
//! reader EOF 时 respawn，init 卡死但未退出时无法检测）。

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{Mutex, RwLock, oneshot};
use tracing::{debug, error, info, warn};

/// respawn 冷却时间：两次 respawn 之间至少间隔 30s，防止 init 反复崩溃
/// 导致 respawn 风暴。冷却期间 init_call 返 NotConnected 错。
const RESPAWN_COOLDOWN: Duration = Duration::from_secs(30);

/// JSON-RPC 2.0 请求帧（control→init）。
#[derive(Debug, Clone, serde::Serialize)]
struct Request {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 响应帧（init→control）。
#[derive(Debug, Clone, serde::Deserialize)]
struct Response {
    id: u64,
    #[serde(default)]
    result: serde_json::Value,
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RpcError {
    code: i32,
    message: String,
}

/// IPC client to cocktail-init subprocess.
pub struct InitClient {
    inner: Arc<Inner>,
    /// 持有 child，drop 时自动 kill。
    _child: Child,
}

struct Inner {
    stdin: Mutex<BufWriter<ChildStdin>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Response>>>,
    next_id: Mutex<u64>,
}

/// init 子进程 supervisor：持有可更新句柄，崩溃后自动 respawn。
///
/// 生命周期：
/// - `spawn_initial` 首次启动时由 `try_spawn_init` 调用一次；
/// - init 崩溃后 reader_loop 收到 EOF，调 `handle_exit` 清空 current 并
///   在冷却时间外 respawn 新进程 + 重新握手 master key；
/// - control 退出时调 `shutdown` 主动关闭 stdin pipe，init 端 reader EOF
///   自然退出（不需要 kill_on_drop）。
pub struct InitSupervisor {
    /// 当前 init client 句柄。None 表示 init 未启动 / 正在 respawn / 已 shutdown。
    /// RwLock 让 init_call 读不阻塞 respawn 写。
    current: RwLock<Option<Arc<InitClient>>>,
    /// 上次 respawn 时间，用于 30s 冷却限流。
    last_respawn: Mutex<Option<Instant>>,
    /// 累计 respawn 次数，用于日志诊断。
    respawn_count: AtomicU64,
}

impl InitSupervisor {
    /// 创建 supervisor。调用方拿到 Arc 后再调 `spawn_initial(&Arc)` 启动子进程。
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            current: RwLock::new(None),
            last_respawn: Mutex::new(None),
            respawn_count: AtomicU64::new(0),
        })
    }

    /// 首次启动 init 子进程并握手 master key。成功后 current=Some(client)。
    /// 握手失败仅记日志（init 仍可用，secrets fallback 到本地）。
    pub async fn spawn_initial(self: &Arc<Self>) -> io::Result<()> {
        let client = InitClient::spawn(Arc::downgrade(self)).await?;
        *self.current.write().await = Some(Arc::new(client));
        self.handshake_master_key().await;
        Ok(())
    }

    /// 调一个 RPC method。current 为 None 时返 NotConnected（init 重启中 / 已 shutdown）。
    pub async fn call<P: Serialize>(
        &self,
        method: &str,
        params: P,
    ) -> io::Result<serde_json::Value> {
        let client = self.get().await?;
        client.call(method, params).await
    }

    /// 取当前 client 句柄。None 时返 NotConnected。
    pub async fn get(&self) -> io::Result<Arc<InitClient>> {
        self.current
            .read()
            .await
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "init not available (respawning or shutdown)"))
    }

    /// 便捷封装：调 get_master_key，返回 32 字节密钥。
    pub async fn get_master_key(&self) -> io::Result<Vec<u8>> {
        let client = self.get().await?;
        client.get_master_key().await
    }

    /// reader_loop EOF 时调用：清空 current，触发 respawn（如未冷却）。
    /// 此方法由 reader task 的 EOF 分支 `tokio::spawn` 调用，必须 self: Arc
    /// 持有所有权，避免 supervisor 在 respawn 期间被 drop。
    pub async fn handle_exit(self: Arc<Self>) {
        // 清空 current，让 init_call 立即返 NotConnected
        let dropped = self.current.write().await.take();
        if dropped.is_none() {
            // 已经被清空（可能 shutdown 主动触发），不再 respawn
            return;
        }
        info!(pid = ?std::process::id(), "cocktail-init subprocess exited; evaluating respawn");

        // 冷却检查：30s 内不重复 respawn
        let now = Instant::now();
        {
            let mut last = self.last_respawn.lock().await;
            if let Some(t) = *last {
                let elapsed = now.duration_since(t);
                if elapsed < RESPAWN_COOLDOWN {
                    let remain = RESPAWN_COOLDOWN - elapsed;
                    warn!(
                        remain_secs = remain.as_secs(),
                        "init respawn skipped: cooldown (recent crash within 30s)"
                    );
                    return;
                }
            }
            *last = Some(now);
        }

        // respawn 新进程
        let count = self.respawn_count.fetch_add(1, Ordering::Relaxed) + 1;
        info!(respawn_count = count, "respawning cocktail-init subprocess");
        match InitClient::spawn(Arc::downgrade(&self)).await {
            Ok(client) => {
                *self.current.write().await = Some(Arc::new(client));
                // 重新握手 master key（init 从文件读同一份 key，set_init_key
                // 已 set 时丢弃新值，但值相同所以无影响）
                self.handshake_master_key().await;
                info!(respawn_count = count, "cocktail-init respawn succeeded");
            }
            Err(e) => {
                error!(error = %e, respawn_count = count, "init respawn failed; will retry on next EOF (no auto-retry without watchdog)");
            }
        }
    }

    /// 主动 shutdown：关闭 stdin pipe，init 端 reader 收到 EOF 自然退出。
    /// 由 control graceful shutdown 调用。幂等（已 shutdown 时 current=None）。
    pub async fn shutdown(&self) {
        let dropped = self.current.write().await.take();
        if let Some(client) = dropped {
            // drop client 会关闭 BufWriter<ChildStdin>，进而关闭 stdin pipe。
            // init 端 server.rs::run 读到 EOF 后 break，正常退出。
            drop(client);
            debug!("init shutdown: stdin pipe closed, subprocess will exit on EOF");
        }
    }

    /// 与 init 子进程握手拉 master key 并注入 secrets。失败仅记日志
    /// （init 仍可用，secrets fallback 到本地 load_or_create）。
    async fn handshake_master_key(&self) {
        match self.get_master_key().await {
            Ok(key) if key.len() == 32 => {
                tracing::info!(
                    source = "init-rpc",
                    "master key loaded from cocktail-init subprocess"
                );
                crate::secrets::set_init_key(key);
            }
            Ok(other) => {
                tracing::warn!(
                    len = other.len(),
                    "cocktail-init returned master key with unexpected length; fallback to local file"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "cocktail-init get_master_key failed; fallback to local file");
            }
        }
    }
}

/// 找 cocktail-init 可执行文件：env COCKTAIL_INIT_PATH > 当前 exe 同目录 > PATH。
fn resolve_init_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("COCKTAIL_INIT_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) {
        "cocktail-init.exe"
    } else {
        "cocktail-init"
    };
    let candidate = dir.join(name);
    if candidate.exists() {
        return Some(candidate);
    }
    // PATH 查找
    if let Ok(paths) = std::env::var("PATH") {
        for entry in paths.split(if cfg!(windows) { ';' } else { ':' }) {
            let p = PathBuf::from(entry).join(name);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

impl InitClient {
    /// fork+exec cocktail-init 子进程，建立 stdin/stdout pipe，启动 reader task。
    /// `weak_supervisor` 用于 reader EOF 时通知 supervisor 触发 respawn。
    pub async fn spawn(weak_supervisor: Weak<InitSupervisor>) -> io::Result<Self> {
        let bin = resolve_init_binary().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "cocktail-init binary not found (checked COCKTAIL_INIT_PATH, current exe dir, PATH)",
            )
        })?;
        debug!(path = %bin.display(), "spawning cocktail-init subprocess");

        let mut cmd = tokio::process::Command::new(&bin);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()); // init 的 stderr 日志直接继承，由 control 的终端显示

        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "init stdin not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "init stdout not piped"))?;

        let inner = Arc::new(Inner {
            stdin: Mutex::new(BufWriter::new(stdin)),
            pending: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
        });

        // 启动 reader task：循环读帧，按 id 路由到 pending oneshot。
        // reader EOF 时通过 weak_supervisor 通知 supervisor 触发 respawn
        // （cooldown 内不重复 respawn）。weak ref 避免 reader task 持有
        // supervisor 导致循环引用。
        let reader_inner = Arc::clone(&inner);
        tokio::spawn(async move {
            match reader_loop(reader_inner, stdout).await {
                Ok(()) => {
                    // 正常 EOF：尝试通知 supervisor respawn（如未 cooldown）
                    if let Some(sup) = weak_supervisor.upgrade() {
                        tokio::spawn(async move { sup.handle_exit().await; });
                    }
                }
                Err(e) => {
                    warn!(error = %e, "init reader task ended with error");
                    if let Some(sup) = weak_supervisor.upgrade() {
                        tokio::spawn(async move { sup.handle_exit().await; });
                    }
                }
            }
        });

        Ok(Self {
            inner,
            _child: child,
        })
    }

    /// 调一个 RPC method，返回 result 或 Err（IPC 失败 / RPC error）。
    pub async fn call<P: Serialize>(
        &self,
        method: &str,
        params: P,
    ) -> io::Result<serde_json::Value> {
        let id = {
            let mut counter = self.inner.next_id.lock().await;
            let id = *counter;
            *counter += 1;
            id
        };
        let params = serde_json::to_value(params).unwrap_or(serde_json::Value::Null);
        let req = Request {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params: Some(params),
        };

        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().await.insert(id, tx);

        let body = serde_json::to_vec(&req).unwrap();
        let frame = frame_bytes(&body);
        {
            let mut w = self.inner.stdin.lock().await;
            w.write_all(&frame).await?;
            w.flush().await?;
        }

        let resp = match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => {
                self.inner.pending.lock().await.remove(&id);
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "init reader dropped response sender",
                ));
            }
            Err(_) => {
                self.inner.pending.lock().await.remove(&id);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "init RPC timed out after 30s",
                ));
            }
        };

        if let Some(err) = resp.error {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("init RPC error [{}]: {}", err.code, err.message),
            ));
        }
        Ok(resp.result)
    }

    /// 便捷封装：调 get_master_key，返回 32 字节密钥。
    /// 调用方在失败时 fallback 到本地 load_or_create。
    pub async fn get_master_key(&self) -> io::Result<Vec<u8>> {
        let v = self
            .call("secrets.get_master_key", serde_json::Value::Null)
            .await?;
        let hex = v
            .get("key_hex")
            .and_then(|h| h.as_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing key_hex"))?;
        hex_decode(hex)
    }
}

/// reader task：循环读帧，按 id 路由到 pending oneshot。
async fn reader_loop(inner: Arc<Inner>, stdout: ChildStdout) -> io::Result<()> {
    let mut reader = BufReader::new(stdout);
    loop {
        let frame = match read_frame(&mut reader).await {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                debug!("init stdout EOF (subprocess exiting)");
                // 通知所有 pending 请求失败
                let mut pending = inner.pending.lock().await;
                pending.clear();
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        let resp: Response = match serde_json::from_slice(&frame) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "failed to parse init response frame");
                continue;
            }
        };
        if let Some(tx) = inner.pending.lock().await.remove(&resp.id) {
            let _ = tx.send(resp);
        }
    }
}

/// 读一帧：4 字节 big-endian length + body。
async fn read_frame<R: AsyncReadExt + Unpin>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    const MAX_FRAME: usize = 64 * 1024 * 1024;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame length {len} exceeds max {MAX_FRAME}"),
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

/// 组帧：4 字节 length + body。
fn frame_bytes(body: &[u8]) -> Vec<u8> {
    let len = body.len() as u32;
    let mut out = len.to_be_bytes().to_vec();
    out.extend_from_slice(body);
    out
}

fn hex_decode(s: &str) -> io::Result<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "odd hex length"));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = hex_digit(bytes[i])?;
        let lo = hex_digit(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_digit(b: u8) -> io::Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "bad hex digit")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let body = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}";
        let frame = frame_bytes(body);
        assert_eq!(frame.len(), body.len() + 4);
        // 前 4 字节是 length big-endian
        let len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]);
        assert_eq!(len as usize, body.len());
    }

    #[test]
    fn hex_decode_basic() {
        assert_eq!(hex_decode("0011ff").unwrap(), vec![0x00, 0x11, 0xff]);
        assert!(hex_decode("xyz").is_err());
        assert!(hex_decode("0").is_err());
    }
}
