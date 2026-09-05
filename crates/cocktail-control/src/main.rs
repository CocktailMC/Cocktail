//! Cocktail Manager control plane — v0.1 (26Q3)

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cocktail_control::run_plane().await
}
