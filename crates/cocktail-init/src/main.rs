//! cocktail-init 入口：从 stdin 读 JSON-RPC 帧，dispatch，写 Response 到 stdout。
//!
//! 日志走 stderr（control 端转发到自己的 tracing）。
//! control 退出时关闭 stdin，本进程读到 EOF 自然退出。

use cocktail_init::server::Server;
use cocktail_init::{proto, rcon, secrets, sevenz};

use serde::Deserialize;

/// sevenz.extract RPC 参数。
#[derive(Debug, Deserialize)]
struct ExtractParams {
    archive: String,
    dest: String,
}

/// sevenz.is_supported_name RPC 参数。
#[derive(Debug, Deserialize)]
struct NameParams {
    name: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    // 日志走 stderr（control 端转发到自己的 tracing）。
    // 阶段 1 用最简初始化：RUST_LOG=info 时输出到 stderr。
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    tracing::info!("cocktail-init starting (pid={})", std::process::id());

    let mut server = Server::new();

    // secrets.get_master_key：返回 32 字节密钥（hex 字符串形式，避免 JSON 字节数组麻烦）
    server.register("secrets.get_master_key", |_params| async move {
        match secrets::get_master_key() {
            Ok(key) => Ok(serde_json::json!({
                "key_hex": hex_encode(&key),
                "source": secrets::key_source(),
            })),
            Err(msg) => Err(proto::Error::internal(msg)),
        }
    });

    // secrets.key_source：返回密钥来源描述
    server.register("secrets.key_source", |_params| async move {
        Ok(serde_json::json!({ "source": secrets::key_source() }))
    });

    // sevenz.ensure_bin：落地内置 7z 并返回路径。失败返 RPC error。
    server.register("sevenz.ensure_bin", |_params| async move {
        match sevenz::ensure_bin() {
            Ok(path) => {
                let embedded = sevenz::EMBEDDED.is_some();
                Ok(serde_json::json!({
                    "path": path.to_string_lossy(),
                    "embedded": embedded,
                }))
            }
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // sevenz.extract：解压 archive 到 dest。params: { archive, dest }
    server.register("sevenz.extract", |params| async move {
        let p: ExtractParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        match sevenz::extract(std::path::Path::new(&p.archive), std::path::Path::new(&p.dest)) {
            Ok(()) => Ok(serde_json::Value::Null),
            Err(e) => Err(proto::Error::internal(format!("{e}"))),
        }
    });

    // sevenz.is_supported_name：纯字符串校验，params: { name }
    server.register("sevenz.is_supported_name", |params| async move {
        let p: NameParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        Ok(serde_json::json!({ "supported": sevenz::is_supported_name(&p.name) }))
    });

    // rcon.try_rcon：TCP 连接测试
    server.register("rcon.try_rcon", |params| async move {
        let params: rcon::TryRconParams = match serde_json::from_value(params) {
            Ok(v) => v,
            Err(e) => return Err(proto::Error::invalid_params(format!("{e}"))),
        };
        let r = rcon::try_rcon(params).await;
        Ok(serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
    });

    // 用 stdin/stdout 跑主循环
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    server.run(stdin, stdout).await
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}
