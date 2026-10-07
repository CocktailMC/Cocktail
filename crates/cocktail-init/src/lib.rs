//! Cocktail-init：用户空间操作执行者。
//!
//! cocktail-init 是 cocktail-control 在启动时 fork+exec 拉起的子进程，
//! 通过 stdin/stdout pipe + 长度前缀 JSON-RPC 2.0 与 control 双向通信。
//! 设计参考 systemd --user 的用户态哲学：不依赖系统特权，不装 systemd unit
//! 或 Windows Service，以当前登录用户身份运行。
//!
//! 阶段 1 范围：IPC 协议骨架 + secrets 双面（master.key 文件管理）+ ensure_7z
//! + try_rcon 验证管线。

pub mod files;
pub mod http;
pub mod java;
pub mod proto;
pub mod rcon;
pub mod secrets;
pub mod server;
pub mod sevenz;
pub mod versions;

pub use proto::{Error, Event, Request, Response, RpcResult};
