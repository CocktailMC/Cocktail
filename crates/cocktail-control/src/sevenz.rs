//! 7z 操作客户端：阶段 2 起实际逻辑搬到 cocktail-init 子进程。
//!
//! 本模块只做 RPC 客户端封装：`ensure_bin` 与 `extract` 转发到 init 端
//! `sevenz.ensure_bin` / `sevenz.extract`。`is_supported_name` 是纯字符串
//! 校验，沉到 cocktail-shared 两侧复用。
//!
//! TODO 阶段 3：init 不可达时考虑 fallback 到本地 spawn 7z（保留旧实现）。

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub use cocktail_shared::sevenz::is_supported_name;

#[derive(Debug, Deserialize)]
struct EnsureBinResult {
    path: String,
    #[allow(dead_code)]
    embedded: bool,
}

/// 落地内置 7z 并返回路径。实际工作在 cocktail-init 子进程内完成。
pub async fn ensure_bin() -> anyhow::Result<PathBuf> {
    let v = crate::init_call("sevenz.ensure_bin", serde_json::Value::Null).await?;
    let r: EnsureBinResult = serde_json::from_value(v)
        .map_err(|e| anyhow::anyhow!("parse sevenz.ensure_bin response: {e}"))?;
    Ok(PathBuf::from(r.path))
}

/// 解压 archive 到 dest。dest 不存在会自动创建。
pub async fn extract(archive: &Path, dest: &Path) -> anyhow::Result<()> {
    #[derive(serde::Serialize)]
    struct Params<'a> {
        archive: &'a str,
        dest: &'a str,
    }
    let params = Params {
        archive: &archive.to_string_lossy(),
        dest: &dest.to_string_lossy(),
    };
    crate::init_call("sevenz.extract", params).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_common_pack_names() {
        assert!(is_supported_name("server.7z"));
        assert!(is_supported_name("pack.TAR.GZ"));
        assert!(!is_supported_name("notes.txt"));
    }
}
