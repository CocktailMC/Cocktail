//! JSON-RPC 2.0 协议消息定义。
//!
//! 传输层：长度前缀帧（4 字节 big-endian u32 length + JSON body），通过
//! stdin/stdout pipe 双向传输。control→init 是 Request，init→control 是
//! Response（对 Request 的回应）或 Event（id 为 null 的主动推送）。

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 请求：control→init 的方法调用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 响应：init→control 对 Request 的回应。
/// result 与 error 互斥：成功用 result，失败用 error。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: serde_json::Value,
    #[serde(default)]
    pub error: Option<Error>,
}

/// JSON-RPC 2.0 事件：init→control 的主动推送（id 固定为 0，与请求区分）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub jsonrpc: String,
    /// 事件推送固定 id=0；正常请求 id 从 1 开始。
    #[serde(default)]
    pub id: Option<u64>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl Error {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self::new(-32601, format!("method not found: {method}"))
    }

    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self::new(-32602, msg)
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::new(-32603, msg)
    }
}

/// RPC handler 返回值：成功返回 serde_json::Value，失败返回 Error。
pub type RpcResult = Result<serde_json::Value, Error>;

impl Response {
    pub fn success(id: u64, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result,
            error: None,
        }
    }

    pub fn error(id: u64, err: Error) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: serde_json::Value::Null,
            error: Some(err),
        }
    }
}

impl Event {
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id: None,
            method: method.into(),
            params,
        }
    }
}
