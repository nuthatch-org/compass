use serde::{Deserialize, Serialize};

/// MCP protocol version implemented by this server.
pub const PROTOCOL_VERSION: &str = "2025-03-26";

/// Inbound JSON-RPC 2.0 request from an agent.
#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Outbound JSON-RPC 2.0 response to the agent.
#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub jsonrpc: &'static str,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Serialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl McpResponse {
    pub fn ok(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self { jsonrpc: "2.0", id, result: Some(result), error: None }
    }

    pub fn err(id: Option<serde_json::Value>, error: McpError) -> Self {
        Self { jsonrpc: "2.0", id, result: None, error: Some(error) }
    }
}

// ----- Standard error constructors -----

pub fn method_not_found(method: &str) -> McpError {
    McpError {
        code: -32601,
        message: format!("method not found: {method}"),
        data: None,
    }
}

pub fn invalid_params(msg: &str) -> McpError {
    McpError { code: -32602, message: msg.to_string(), data: None }
}

pub fn internal_error(msg: &str) -> McpError {
    McpError { code: -32603, message: msg.to_string(), data: None }
}

/// Non-standard error (-32001) returned when no valid TAP receipt was provided.
/// `data` carries the x402 payment spec so the agent knows how to pay.
pub fn payment_required_error(data: serde_json::Value) -> McpError {
    McpError {
        code: -32001,
        message: "payment required — include a signed TAP-Receipt header".to_string(),
        data: Some(data),
    }
}
