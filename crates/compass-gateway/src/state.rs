use std::sync::Arc;

use alloy_primitives::B256;
use compass_core::config::Config;

/// Shared application state — cheaply cloneable, lives for the process lifetime.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub http_client: reqwest::Client,
    /// Pre-computed EIP-712 domain separator for GraphTallyCollector on Arbitrum One.
    pub tap_domain_sep: B256,
}
