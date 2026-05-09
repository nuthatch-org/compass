//! MCP (Model Context Protocol) Streamable HTTP protocol types.
//!
//! Implements the wire format for the Streamable HTTP transport from the
//! MCP 2025-03-26 specification. Agents POST JSON-RPC 2.0 messages to the
//! single `/` endpoint; the server responds with `application/json`.
//!
//! SSE streaming (`text/event-stream`) is a week-4 addition — for now all
//! responses are single JSON objects.

pub mod protocol;

pub use protocol::{
    internal_error, invalid_params, method_not_found, payment_required_error, McpError,
    McpRequest, McpResponse, PROTOCOL_VERSION,
};
