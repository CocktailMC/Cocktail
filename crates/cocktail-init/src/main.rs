//! cocktail-init 入口：从 stdin 读 JSON-RPC 帧，dispatch，写 Response 到 stdout。
//!
//! 日志走 stderr（control 端转发到自己的 tracing）。
//! control 退出时关闭 stdin，本进程读到 EOF 自然退出。
//!
//! 全部 method 注册在 [`cocktail_init::rpc::build_server`]，子进程模式与
//! control 侧进程内 fallback 共用同一份实现。

use cocktail_init::build_server;

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    // 日志走 stderr（control 端转发到自己的 tracing）。
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .event_format(cocktail_shared::logging::CocktailFormat)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();

    tracing::info!("cocktail-init starting (pid={})", std::process::id());

    // 用 stdin/stdout 跑主循环
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    build_server().run(stdin, stdout).await
}
