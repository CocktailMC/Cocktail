//! cocktail-init 子进程 IPC client。
//!
//! `InitClient` 是 JSON-RPC over stdin/stdout 的 **IPC 传输原语**：负责 fork+exec
//! 子进程、4 字节 big-endian 长度前缀组帧、按 id 路由响应。
//!
//! 生命周期管理（spawn / respawn / 停止）已上移到通用
//! [`crate::supervisor::ServiceSupervisor`]：`InitClient::spawn` 只负责建立 pipe
//! 与 reader task，并把 `Child` 所有权交回 supervisor 做 watcher。
//!
//! fallback 设计：spawn 或 call 失败时返回 Err，调用方退化到本地函数
//! （如 secrets::load_or_create 直接读文件），保证 init 不可达时
//! control 仍能鉴权与运行（兼容单进程老部署）。

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{Mutex, broadcast, oneshot};
use tracing::{debug, warn};

use cocktail_shared::proto::Event;

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

/// IPC client to cocktail-init subprocess。
///
/// stdout 是 RPC 应答通道（不可用于抓日志）；日志走 stderr，由 supervisor 采集。
pub struct InitClient {
    inner: Arc<Inner>,
}

struct Inner {
    /// `None` 表示 stdin 已关闭（优雅停止或管道断开）。
    stdin: Mutex<Option<BufWriter<ChildStdin>>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Response>>>,
    next_id: Mutex<u64>,
    /// init→control 主动推送事件（Event 帧）的广播出口；订阅者按需 subscribe。
    events: broadcast::Sender<Event>,
}

/// 一帧的 JSON-RPC 形状分类结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// init→control 对某次 Request 的回应（有 id，含 result 或 error）。
    Response,
    /// init→control 主动推送（有 method，无 result/error）。
    Event,
    /// 不符合任一形状：记 warn 并丢弃，不 panic。
    Invalid,
}

/// 判定一帧是 Response 还是 Event（纯函数，JSON-RPC 2.0 形状判据）。
///
/// - 含 `method` 且不含 `result`/`error` → [`FrameKind::Event`]（notification 形状）；
/// - 含 `id` 且含 `result` 或 `error` → [`FrameKind::Response`]；
/// - 其余 → [`FrameKind::Invalid`]。
pub fn classify_frame(v: &serde_json::Value) -> FrameKind {
    let has_method = v.get("method").is_some();
    let has_result = v.get("result").is_some();
    let has_error = v.get("error").is_some();
    let has_id = v.get("id").is_some();
    if has_method && !has_result && !has_error {
        FrameKind::Event
    } else if has_id && (has_result || has_error) {
        FrameKind::Response
    } else {
        FrameKind::Invalid
    }
}

impl InitClient {
    /// 启动子进程并建立 stdin/stdout pipe，返回 `(client, Child, stderr)`。
    ///
    /// `cmd` 由集成方（supervisor）构造好 program / args / cwd / env；
    /// 本函数只负责设置 stdio、spawn、启动 reader task，并把 `Child` 与 `stderr`
    /// 交回调用方（supervisor 持有 `Child` 做 watcher，持有 `stderr` 采集日志）。
    pub fn spawn(cmd: &mut Command) -> io::Result<(Arc<Self>, Child, ChildStderr)> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "init stdin not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "init stdout not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "init stderr not piped"))?;

        let (events, _) = broadcast::channel(1024);
        let inner = Arc::new(Inner {
            stdin: Mutex::new(Some(BufWriter::new(stdin))),
            pending: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            events,
        });

        // reader task：循环读帧，按 id 路由到 pending oneshot。EOF 时通知所有
        // pending 失败；进程退出/重启由 supervisor 的 watcher 负责。
        let reader_inner = Arc::clone(&inner);
        tokio::spawn(async move {
            if let Err(e) = reader_loop(reader_inner, stdout).await {
                warn!(error = %e, "init reader task ended with error");
            }
        });

        Ok((Arc::new(Self { inner }), child, stderr))
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
            let mut guard = self.inner.stdin.lock().await;
            let w = guard.as_mut().ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "init stdin already closed")
            })?;
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

    /// 关闭 stdin pipe（drop `BufWriter<ChildStdin>`）。init 端 reader 收到 EOF
    /// 后自然退出，用于优雅停止。幂等。
    pub async fn close_stdin(&self) {
        let taken = self.inner.stdin.lock().await.take();
        if taken.is_some() {
            debug!("init stdin closed; subprocess will exit on EOF");
        }
    }

    /// 订阅 init 推送的事件（Event 帧）。
    ///
    /// 每次 spawn 生成一个新的广播 channel；init 停止/重启（`Inner` 被 drop）
    /// 后旧 receiver 收到 `Closed`，调用方应重新调用本方法重建订阅。
    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }
}

/// 找 cocktail-init 可执行文件：env COCKTAIL_INIT_PATH > 当前 exe 同目录 > PATH。
pub(crate) fn resolve_init_binary() -> Option<PathBuf> {
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
        let value: serde_json::Value = match serde_json::from_slice(&frame) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "failed to parse init frame as JSON");
                continue;
            }
        };
        match classify_frame(&value) {
            FrameKind::Event => match serde_json::from_value::<Event>(value) {
                Ok(ev) => {
                    // 无人订阅时 send 返回 Err，忽略即可。
                    let _ = inner.events.send(ev);
                }
                Err(e) => warn!(error = %e, "failed to parse init event frame"),
            },
            FrameKind::Response => match serde_json::from_value::<Response>(value) {
                Ok(resp) => {
                    if let Some(tx) = inner.pending.lock().await.remove(&resp.id) {
                        let _ = tx.send(resp);
                    }
                }
                Err(e) => warn!(error = %e, "failed to parse init response frame"),
            },
            FrameKind::Invalid => {
                warn!("discarding init frame with unrecognised JSON-RPC shape");
            }
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

    fn v(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn classify_event_has_method_no_result() {
        // init 序列化的 Event：id=null、有 method、无 result/error。
        assert_eq!(
            classify_frame(&v(
                r#"{"jsonrpc":"2.0","id":null,"method":"download.started","params":{}}"#
            )),
            FrameKind::Event
        );
        // 无 id 的 notification 形状同样是 Event。
        assert_eq!(
            classify_frame(&v(
                r#"{"method":"download.progress","params":{"received":1}}"#
            )),
            FrameKind::Event
        );
    }

    #[test]
    fn classify_response_with_result() {
        // 成功 Response：init 无 skip_serializing_if，error 序列化为 null。
        assert_eq!(
            classify_frame(&v(
                r#"{"jsonrpc":"2.0","id":7,"result":{"ok":true},"error":null}"#
            )),
            FrameKind::Response
        );
    }

    #[test]
    fn classify_response_with_error() {
        assert_eq!(
            classify_frame(&v(
                r#"{"jsonrpc":"2.0","id":7,"result":null,"error":{"code":-32603,"message":"x"}}"#
            )),
            FrameKind::Response
        );
    }

    #[test]
    fn classify_method_plus_result_is_response() {
        // 同时有 method 与 result：按 Response 处理（有 id）。
        assert_eq!(
            classify_frame(&v(r#"{"id":1,"method":"m","result":{}}"#)),
            FrameKind::Response
        );
    }

    #[test]
    fn classify_invalid_shapes() {
        // 无 method、无 result/error → Invalid
        assert_eq!(
            classify_frame(&v(r#"{"jsonrpc":"2.0"}"#)),
            FrameKind::Invalid
        );
        assert_eq!(
            classify_frame(&v(r#"{"jsonrpc":"2.0","id":3}"#)),
            FrameKind::Invalid
        );
        // 有 method 也有 error（既非纯净 Event，也无 id）→ Invalid
        assert_eq!(
            classify_frame(&v(r#"{"method":"m","error":{"code":1}}"#)),
            FrameKind::Invalid
        );
    }

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
