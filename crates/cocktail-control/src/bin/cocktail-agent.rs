#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cocktail_control::agent_runtime::run_agent().await
}
