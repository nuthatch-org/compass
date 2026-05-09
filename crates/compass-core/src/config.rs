use alloy_primitives::Address;
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub gateway: GatewayConfig,
    pub tap: TapConfig,
    pub graph_node: GraphNodeConfig,
    #[serde(default)]
    pub x402: Option<X402Config>,
    #[serde(default)]
    pub subgraphs: Vec<SubgraphConfig>,
}

/// Optional x402 configuration for USDC-on-Base payment validation.
/// If absent, X-Payment headers are rejected and only TAP receipts are accepted.
#[derive(Debug, Clone, Deserialize)]
pub struct X402Config {
    /// Base mainnet (or testnet) JSON-RPC URL used to verify on-chain USDC transfers.
    pub base_rpc_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Indexer staking wallet — used as `service_provider` in TAP receipts.
    pub indexer_address: Address,
    /// Publicly reachable MCP endpoint URL for this indexer.
    pub endpoint_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TapConfig {
    /// MCPDataService contract address on Arbitrum One.
    pub data_service_address: Address,
    /// EIP-712 domain name (default: "GraphTallyCollector").
    #[serde(default = "default_domain_name")]
    pub eip712_domain_name: String,
    /// Chain ID where GraphTallyCollector is deployed (42161 = Arbitrum One).
    #[serde(default = "default_chain_id")]
    pub eip712_chain_id: u64,
    /// GraphTallyCollector contract address on Arbitrum One.
    #[serde(default = "default_tally_collector")]
    pub eip712_verifying_contract: Address,
    /// Default price per MCP tool call in GRT wei.
    /// 10_000_000_000_000 = 0.00001 GRT ≈ $0.000001 at $0.10/GRT.
    #[serde(default = "default_price_per_query")]
    pub price_per_query_grt_wei: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GraphNodeConfig {
    /// Base URL of the local graph-node query endpoint.
    /// Queries are forwarded to `{url}/subgraphs/id/{deployment_id}`.
    pub url: String,
    #[serde(default = "default_gn_timeout")]
    pub timeout_secs: u64,
}

/// A subgraph registered for MCP serving.
/// Each entry appears as one tool in `tools/list`.
#[derive(Debug, Clone, Deserialize)]
pub struct SubgraphConfig {
    /// IPFS deployment CID (e.g. "QmXxx...").
    pub deployment_id: String,
    /// Tool name exposed to agents — must be unique, snake_case recommended.
    pub name: String,
    /// Description shown to agents in tools/list.
    pub description: String,
    /// Network/chain this subgraph indexes (e.g. "mainnet", "arbitrum-one").
    pub network: String,
    /// Per-subgraph price override in GRT wei. Falls back to tap.price_per_query_grt_wei.
    pub price_per_query_grt_wei: Option<u64>,
    /// Per-subgraph graph-node URL override. Falls back to graph_node.url.
    pub graph_node_url: Option<String>,
}

impl SubgraphConfig {
    pub fn effective_price(&self, default: u64) -> u64 {
        self.price_per_query_grt_wei.unwrap_or(default)
    }

    pub fn effective_graph_node_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.graph_node_url.as_deref().unwrap_or(default)
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path =
            std::env::var("COMPASS_CONFIG").unwrap_or_else(|_| "compass.toml".to_string());
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read config from {path}"))?;
        toml::from_str(&contents).context("failed to parse config")
    }
}

fn default_host() -> String { "0.0.0.0".to_string() }
fn default_port() -> u16 { 8090 }
fn default_domain_name() -> String { "GraphTallyCollector".to_string() }
fn default_chain_id() -> u64 { 42161 }
fn default_tally_collector() -> Address {
    "0x8f69F5C07477Ac46FBc491B1E6D91E2bb0111A9e".parse().unwrap()
}
fn default_price_per_query() -> u64 { 10_000_000_000_000 }
fn default_gn_timeout() -> u64 { 30 }
