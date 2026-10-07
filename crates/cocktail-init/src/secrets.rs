//! cocktail-init 端密钥文件管理：读写 data/master.key。
//!
//! 设计：init 只负责"密钥文件读写代理"——读已有文件或生成新文件，
//! 返回 32 字节密钥。HKDF/AEAD/passphrase 派生留 control（env 模式
//! control 自己处理，不走 init）。
//!
//! control 启动握手时调一次 get_master_key RPC 拉取 32 字节密钥，
//! IPC 不可达时 fallback 直接读文件（兼容单进程老部署）。

use std::path::PathBuf;

use rand_core::OsRng;
use rand_core::RngCore;

const KEY_FILE: &str = "data/master.key";

fn key_path() -> PathBuf {
    PathBuf::from(KEY_FILE)
}

/// hex 编码 32 字节 → 64 字符小写 hex。
fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// hex 解码（容错：忽略前后空白）。
fn unhex(s: &str) -> Option<Vec<u8>> {
    let trimmed = s.trim();
    if trimmed.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(trimmed.len() / 2);
    let bytes = trimmed.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = hex_digit(bytes[i])?;
        let lo = hex_digit(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// 读 data/master.key；不存在则 OsRng 生成 32 字节 + 写文件 + 设权限。
/// 失败时返回 Error（由 server 转 RPC error）。
pub fn get_master_key() -> Result<Vec<u8>, String> {
    let path = key_path();

    // 已存在文件：尝试解析 hex
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Some(bytes) = unhex(text.trim()) {
            if bytes.len() == 32 {
                return Ok(bytes);
            }
        }
        // 文件存在但格式坏：不覆盖，返回错误让用户介入
        return Err(format!(
            "master.key exists but is malformed (expected 64-char hex of 32 bytes): {}",
            path.display()
        ));
    }

    // 不存在：生成 + 写
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create data dir: {e}"))?;
    }
    std::fs::write(&path, hex(&key)).map_err(|e| format!("write master.key: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(&path, perms);
        }
    }
    Ok(key.to_vec())
}

/// 密钥来源描述（用于 control 健康检查）。
pub fn key_source() -> &'static str {
    if std::env::var("COCKTAIL_MASTER_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
    {
        "env"
    } else if std::path::Path::new(KEY_FILE).exists() {
        "file"
    } else {
        "generated"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn hex_roundtrip() {
        let bytes = b"\x00\x01\x02\x03\xff\xfe\xfd\xfc";
        assert_eq!(unhex(&hex(bytes)), Some(bytes.to_vec()));
        assert!(unhex("xyz").is_none());
        assert!(unhex("0").is_none());
    }

    #[test]
    fn get_master_key_creates_and_reads() {
        let tmp = tempfile::tempdir().unwrap();
        // 临时切换 cwd（get_master_key 用相对路径 data/master.key）
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        // 清理可能残留的 data/
        let _ = fs::remove_dir_all("data");

        let k1 = get_master_key().expect("first call creates");
        assert_eq!(k1.len(), 32);
        // 第二次读应返回相同密钥
        let k2 = get_master_key().expect("second call reads");
        assert_eq!(k1, k2);

        // 清理
        let _ = fs::remove_dir_all("data");
        std::env::set_current_dir(prev).unwrap();
    }

    #[test]
    fn malformed_key_returns_error_not_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let _ = fs::remove_dir_all("data");
        fs::create_dir_all("data").unwrap();
        fs::write("data/master.key", "not-hex-garbage").unwrap();

        let result = get_master_key();
        assert!(
            result.is_err(),
            "malformed key should not be silently overwritten"
        );
        // 文件内容应该没被覆盖
        let content = fs::read_to_string("data/master.key").unwrap();
        assert_eq!(content, "not-hex-garbage");

        let _ = fs::remove_dir_all("data");
        std::env::set_current_dir(prev).unwrap();
    }
}
