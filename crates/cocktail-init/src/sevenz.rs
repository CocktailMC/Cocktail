//! 7z 二进制存在性检查（阶段 1 验证管线，完整逻辑待阶段 2 搬入）。
//!
//! 阶段 1 只做最小验证：control 通过 IPC 调 ensure_7z，init 检查二进制
//! 是否就位并返回状态。完整的 7z 解压/压缩逻辑留在 control 端，
//! 阶段 2 会搬过来。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ensure7zResult {
    pub present: bool,
    pub path: String,
    pub version: Option<String>,
}

/// 检查 7z 二进制是否就位。阶段 1 返回路径与存在标志，version 留空。
pub fn ensure_7z() -> Ensure7zResult {
    // cocktail-control 现有 sevenz.rs 用 include_bytes! 内置 7z 二进制，
    // 写到 data/7z/7z.exe（Windows）或 data/7z/7z（Unix）。
    let exe = if cfg!(windows) { "7z.exe" } else { "7z" };
    let path = format!("data/7z/{exe}");
    let present = std::path::Path::new(&path).exists();
    Ensure7zResult {
        present,
        path,
        version: None,
    }
}
