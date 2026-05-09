use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use compass_core::config::Config;
use compass_tap::domain_separator;

use crate::routes;
use crate::state::AppState;

pub async fn run(config: Config) -> Result<()> {
    let tap_domain_sep = domain_separator(
        &config.tap.eip712_domain_name,
        config.tap.eip712_chain_id,
        config.tap.eip712_verifying_contract,
    );

    let state = AppState {
        config: Arc::new(config.clone()),
        http_client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.graph_node.timeout_secs))
            .build()?,
        tap_domain_sep,
    };

    let app = routes::router(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr =
        format!("{}:{}", config.gateway.host, config.gateway.port).parse()?;
    tracing::info!(%addr, subgraphs = config.subgraphs.len(), "compass-gateway starting");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
