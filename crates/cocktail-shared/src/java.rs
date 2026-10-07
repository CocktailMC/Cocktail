//! Java 运行时管理共享类型：control plane 与 init 子进程之间 IPC 消息使用。
//!
//! 实际的 JRE 探测/下载/解压/缓存校验逻辑由 cocktail-init 子进程实现，
//! control 端通过 RPC 调用；本模块只沉淀跨进程复用的纯数据类型与
//! `ImageType` 的字符串互转辅助。

use serde::{Deserialize, Serialize};

/// Temurin 镜像类型：JRE 仅运行，JDK 含编译工具。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageType {
    Jre,
    Jdk,
}

impl ImageType {
    pub fn as_str(self) -> &'static str {
        match self {
            ImageType::Jre => "jre",
            ImageType::Jdk => "jdk",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "jre" | "" => Ok(ImageType::Jre),
            "jdk" => Ok(ImageType::Jdk),
            other => anyhow::bail!("image_type 必须是 jre 或 jdk，收到 {other}"),
        }
    }
}

/// 已落地的 JRE/JDK 元数据：写进 `<runtime_dir>/.cocktail.json`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeMeta {
    pub id: String,
    pub vendor: String,
    pub major: u32,
    pub image_type: ImageType,
    pub release_name: String,
    pub os: String,
    pub arch: String,
    pub java_bin: String,
    #[serde(default)]
    pub java_home: String,
}

/// 已安装的运行时：list_installed / find_managed 的返回元素。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledRuntime {
    pub id: String,
    pub vendor: String,
    pub major: u32,
    pub image_type: ImageType,
    pub release_name: String,
    pub java_bin: String,
    pub java_home: String,
    pub size_bytes: u64,
}

/// 系统 PATH 上探测到的 java：`java -version` 解析结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemJava {
    pub java_bin: String,
    pub major: u32,
    pub version: String,
}

/// Java 运行时盘点：inventory() RPC 的返回。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaInventory {
    pub os: String,
    pub arch: String,
    pub adoptium_os: String,
    pub adoptium_arch: String,
    pub system: Option<SystemJava>,
    pub installed: Vec<InstalledRuntime>,
    pub available_lts: Vec<u32>,
    pub recommended_major: u32,
}

/// `java.install` RPC 参数：control 端 HTTP handler 反序列化用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallJavaRequest {
    pub major: u32,
    #[serde(default)]
    pub image_type: Option<String>,
}

/// `java.ensure_api` RPC 参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsureJavaRequest {
    #[serde(default)]
    pub major: Option<u32>,
    #[serde(default)]
    pub image_type: Option<String>,
    #[serde(default)]
    pub managed: bool,
}

/// `java.ensure_api` RPC 返回：指向最终选定的 java 可执行文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsureJavaResponse {
    pub java_bin: String,
    pub java_home: Option<String>,
    pub major: u32,
    pub source: String,
}
