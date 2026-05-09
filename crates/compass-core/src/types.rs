use serde::{Deserialize, Serialize};

/// An MCP tool description returned in `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    /// JSON Schema for the tool's input arguments.
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// x402 payment spec — returned as the body of an HTTP 402 response.
///
/// Follows the x402 protocol: <https://x402.org>
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentSpec {
    pub scheme: String,
    pub network: String,
    #[serde(rename = "maxAmountRequired")]
    pub max_amount_required: String,
    /// The MCP tool name or subgraph deployment ID being paid for.
    pub resource: String,
    pub description: String,
    /// Indexer address that receives the payment.
    #[serde(rename = "payTo")]
    pub pay_to: String,
    /// Payment token contract address.
    pub asset: String,
}
