//! cocktail-init 入口：从 stdin 读 JSON-RPC 帧，dispatch，写 Response 到 stdout。
//!
//! 日志走 stderr（control 端转发到自己的 tracing）。
//! control 退出时关闭 stdin，本进程读到 EOF 自然退出。

use cocktail_init::server::Server;
use cocktail_init::{proto, rcon, secrets, sevenz};

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

    // sevenz.ensure_7z：返回 7z 二进制存在状态
    server.register("sevenz.ensure_7z", |_params| async move {
        let r = sevenz::ensure_7z();
        Ok(serde_json::to_value(r).unwrap_or(serde_json::Value::Null))
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
