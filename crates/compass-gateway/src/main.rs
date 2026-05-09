use anyhow::Result;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    let config = compass_core::config::Config::load()?;
    compass_gateway::server::run(config).await
}
