//! JSON-RPC dispatcher：从 stdin 读帧，路由到 handler，写 Response 到 stdout。
//!
//! 帧格式：4 字节 big-endian u32 length + JSON body。stdout 也用同一帧格式，
//! 既能写 Response（对 Request 的回应）也能写 Event（主动推送）。

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::Mutex;

use crate::proto::{Error, Event, Request, Response, RpcResult};

/// 异步 RPC handler：吃 params，吐 RpcResult。
type Handler = Arc<
    dyn Fn(
            serde_json::Value,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = RpcResult> + Send>>
        + Send
        + Sync,
>;

pub struct Server {
    handlers: HashMap<String, Handler>,
}

impl Server {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// 注册一个 method。handler 是 async fn(Value) -> RpcResult。
    pub fn register<F, Fut>(&mut self, method: impl Into<String>, handler: F)
    where
        F: Fn(serde_json::Value) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = RpcResult> + Send + 'static,
    {
        let h = Arc::new(move |params: serde_json::Value| {
            let f = handler(params);
            Box::pin(f) as std::pin::Pin<Box<dyn std::future::Future<Output = RpcResult> + Send>>
        });
        self.handlers.insert(method.into(), h);
    }

    /// 按 method 路由到 handler。未注册返回 method_not_found。
    async fn dispatch(&self, method: &str, params: serde_json::Value) -> RpcResult {
        match self.handlers.get(method) {
            Some(h) => h(params).await,
            None => Err(Error::method_not_found(method)),
        }
    }

    /// 主循环：从 reader 读帧，dispatch，写 Response 到 writer。
    /// 同时安装全局事件 sink 并起写任务，支持任意 handler 通过
    /// [`crate::events::emit`] 主动推送 Event——Event 与 Response 复用同一
    /// writer 锁，逐帧写入，不会交错撕裂。
    pub async fn run<R, W>(self, reader: R, writer: W) -> io::Result<()>
    where
        R: tokio::io::AsyncRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let server = Arc::new(self);
        let mut reader = BufReader::new(reader);
        let writer = Arc::new(Mutex::new(BufWriter::new(writer)));

        // 安装事件出口：起一个写任务消费 Event 队列，落到同一 writer。
        let (sink, mut event_rx) = crate::events::channel();
        crate::events::install(sink);
        {
            let writer = Arc::clone(&writer);
            tokio::spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    if let Err(e) = Server::write_event(&writer, event).await {
                        tracing::warn!(error = %e, "failed to write init event frame");
                        break;
                    }
                }
            });
        }

        loop {
            let frame = match read_frame(&mut reader).await {
                Ok(frame) => frame,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    // stdin 关闭，control 退出，正常退出
                    break;
                }
                Err(e) => return Err(e),
            };

            let req: Request = match serde_json::from_slice(&frame) {
                Ok(r) => r,
                Err(e) => {
                    let err = Error::new(-32700, format!("parse error: {e}"));
                    let resp = Response::error(0, err);
                    write_frame(&writer, &serde_json::to_vec(&resp)?).await?;
                    continue;
                }
            };

            let id = req.id;
            let result = server.dispatch(&req.method, req.params).await;
            let resp = match result {
                Ok(v) => Response::success(id, v),
                Err(e) => Response::error(id, e),
            };
            write_frame(&writer, &serde_json::to_vec(&resp)?).await?;
        }

        Ok(())
    }

    /// 主动推送一个 Event 到 writer（用于日志/指标/实例事件流）。
    pub async fn write_event<W>(writer: &Arc<Mutex<BufWriter<W>>>, event: Event) -> io::Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        let body = serde_json::to_vec(&event)?;
        write_frame(writer, &body).await
    }
}

/// 读一帧：4 字节 big-endian length + length 字节 body。
pub async fn read_frame<R>(reader: &mut R) -> io::Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    // 防止恶意帧长度爆内存：单帧上限 64 MiB（足以容纳大文件 chunk + 元数据）
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

/// 写一帧：4 字节 big-endian length + body。
pub async fn write_frame<W>(writer: &Arc<Mutex<BufWriter<W>>>, body: &[u8]) -> io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let len = body.len() as u32;
    let mut w = writer.lock().await;
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(body).await?;
    w.flush().await?;
    Ok(())
}

/// 反序列化 params 的辅助函数：从 serde_json::Value 提取强类型参数。
pub fn parse_params<T: DeserializeOwned>(params: serde_json::Value) -> Result<T, Error> {
    serde_json::from_value(params)
        .map_err(|e| Error::invalid_params(format!("invalid params: {e}")))
}
