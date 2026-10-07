//! 服务端核心版本管理共享类型：control plane 与 init 子进程之间 IPC 消息使用。
//!
//! 实际的版本元数据抓取 / 服务端 jar 下载 / Forge / Fabric / NeoForge 安装器
//! 调用都由 cocktail-init 子进程实现，control 端通过 RPC 调用；
//! 本模块只沉淀跨进程复用的纯数据类型。

use serde::{Deserialize, Serialize};

/// 单个核心版本条目（list_versions 返回元素）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreVersion {
    pub id: String,
    pub core: String,
    pub latest: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// 单个 loader 条目（list_loaders 返回元素）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreLoader {
    pub id: String,
    pub latest: bool,
    pub recommended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// `versions.download_and_install` HTTP API 请求体（control 端 axum handler 反序列化）。
#[derive(Debug, Clone, Deserialize)]
pub struct InstallRequest {
    pub core: String,
    pub version: String,
    #[serde(default)]
    pub loader: Option<String>,
}
