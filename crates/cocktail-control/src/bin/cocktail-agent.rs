//! Cocktail node agent — connects to the control plane and runs assigned instances.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cocktail_control::agent_runtime::run_agent().await
}
