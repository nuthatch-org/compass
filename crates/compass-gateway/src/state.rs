use std::collections::HashMap;
use std::sync::Arc;

use alloy_primitives::B256;
use compass_core::config::Config;
use compass_schema::FieldArg;

/// Routing target for a single introspected MCP tool.
pub struct ToolRoute {
    /// Matches `SubgraphConfig.name` — used to look up endpoint, price, etc.
    pub subgraph_name: String,
    /// GraphQL root query field to call.
    pub field_name: String,
    /// Arg metadata — needed to correctly build the GraphQL query (enum quoting, etc.).
    pub args: Vec<FieldArg>,
}

/// Shared application state — cheaply cloneable, lives for the process lifetime.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub http_client: reqwest::Client,
    /// Pre-computed EIP-712 domain separator for GraphTallyCollector on Arbitrum One.
    pub tap_domain_sep: B256,
    /// Pre-built `tools/list` response payload (populated at startup via introspection).
    pub tool_list: Arc<Vec<serde_json::Value>>,
    /// Maps tool name → routing info for introspected (typed) tools.
    /// Generic (raw-query) tools are NOT in this map — they are looked up by
    /// matching `SubgraphConfig.name` directly.
    pub tool_routes: Arc<HashMap<String, ToolRoute>>,
}
