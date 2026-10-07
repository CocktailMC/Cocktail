//! RCON 连接测试（阶段 1 验证管线，完整协议待阶段 2 搬入）。
//!
//! 阶段 1 只做最小验证：control 通过 IPC 调 try_rcon，init 尝试 TCP
//! 连接到给定 host:port，返回连接是否成功。完整的 RCON 协议（认证、
//! 命令发送、响应解析）留在 control 端，阶段 2 会搬过来。

use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::timeout;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TryRconParams {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TryRconResult {
    pub connected: bool,
    pub error: Option<String>,
}

/// 尝试 TCP 连接到 RCON 端口（3s 超时）。阶段 1 不实现完整 RCON 协议。
pub async fn try_rcon(params: TryRconParams) -> TryRconResult {
    let addr = format!("{}:{}", params.host, params.port);
    match timeout(Duration::from_secs(3), TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => TryRconResult {
            connected: true,
            error: None,
        },
        Ok(Err(e)) => TryRconResult {
            connected: false,
            error: Some(format!("connect {addr}: {e}")),
        },
        Err(_) => TryRconResult {
            connected: false,
            error: Some(format!("connect {addr}: timed out after 3s")),
        },
    }
}
