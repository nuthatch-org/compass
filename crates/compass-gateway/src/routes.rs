use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde_json::json;

use compass_mcp::protocol::{
    internal_error, invalid_params, method_not_found, payment_required_error, McpRequest,
    McpResponse, PROTOCOL_VERSION,
};
use compass_schema::generic_tool;
use compass_tap::verify_receipt;

use crate::state::AppState;

/// TAP receipt HTTP header name (lowercase; axum normalises on lookup).
const TAP_RECEIPT_HEADER: &str = "tap-receipt";

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", post(mcp_handler))
        .route("/health", get(health))
        .with_state(state)
}

// ---- Health ----------------------------------------------------------------

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "compass-gateway" }))
}

// ---- MCP Streamable HTTP entrypoint ----------------------------------------

async fn mcp_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<McpRequest>,
) -> Response {
    match req.method.as_str() {
        "initialize" => handle_initialize(req.id).into_response(),
        "ping"       => handle_ping(req.id).into_response(),
        "tools/list" => handle_tools_list(&state, req.id).into_response(),
        "tools/call" => handle_tools_call(&state, req.id, &req.params, &headers).await,
        method => Json(McpResponse::err(req.id, method_not_found(method))).into_response(),
    }
}

// ---- initialize ------------------------------------------------------------

fn handle_initialize(id: Option<serde_json::Value>) -> Json<McpResponse> {
    Json(McpResponse::ok(
        id,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "compass",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    ))
}

// ---- ping ------------------------------------------------------------------

fn handle_ping(id: Option<serde_json::Value>) -> Json<McpResponse> {
    Json(McpResponse::ok(id, json!({})))
}

// ---- tools/list ------------------------------------------------------------

fn handle_tools_list(state: &AppState, id: Option<serde_json::Value>) -> Json<McpResponse> {
    let tools: Vec<serde_json::Value> = state
        .config
        .subgraphs
        .iter()
        .map(|s| {
            let tool = generic_tool(&s.name, &s.description, &s.network);
            let price = s.effective_price(state.config.tap.price_per_query_grt_wei);
            // Embed pricing metadata in the tool description so agents can see the cost.
            let annotated_description = format!(
                "{}\n\nPrice: {} GRT wei per query | deployment: {} | network: {}",
                tool.description, price, s.deployment_id, s.network
            );
            json!({
                "name": tool.name,
                "description": annotated_description,
                "inputSchema": tool.input_schema
            })
        })
        .collect();

    Json(McpResponse::ok(id, json!({ "tools": tools })))
}

// ---- tools/call ------------------------------------------------------------

async fn handle_tools_call(
    state: &AppState,
    id: Option<serde_json::Value>,
    params: &serde_json::Value,
    headers: &HeaderMap,
) -> Response {
    // --- 1. Resolve the requested tool → subgraph config ---
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return Json(McpResponse::err(id, invalid_params("missing 'name' in params")))
                .into_response();
        }
    };

    let subgraph = match state.config.subgraphs.iter().find(|s| s.name == tool_name) {
        Some(s) => s,
        None => {
            return Json(McpResponse::err(
                id,
                invalid_params(&format!("unknown tool: {tool_name}")),
            ))
            .into_response();
        }
    };

    let price = subgraph.effective_price(state.config.tap.price_per_query_grt_wei);
    let indexer_addr = state.config.gateway.indexer_address.to_string();

    // --- 2. Check payment (TAP receipt required) ---
    match headers.get(TAP_RECEIPT_HEADER) {
        None => {
            // No TAP receipt — return HTTP 402 with x402 payment spec.
            let body = compass_x402::payment_required_body(tool_name, price, &indexer_addr);
            return (StatusCode::PAYMENT_REQUIRED, Json(body)).into_response();
        }
        Some(raw) => {
            let header_str = match raw.to_str() {
                Ok(s) => s,
                Err(_) => {
                    return Json(McpResponse::err(
                        id,
                        invalid_params("tap-receipt header is not valid UTF-8"),
                    ))
                    .into_response();
                }
            };

            let signed_receipt: compass_tap::SignedReceipt =
                match serde_json::from_str(header_str) {
                    Ok(r) => r,
                    Err(e) => {
                        return Json(McpResponse::err(
                            id,
                            invalid_params(&format!("tap-receipt parse error: {e}")),
                        ))
                        .into_response();
                    }
                };

            if let Err(e) = verify_receipt(
                &signed_receipt,
                state.tap_domain_sep,
                state.config.tap.data_service_address,
                state.config.gateway.indexer_address,
                price,
            ) {
                tracing::warn!(%tool_name, error = %e, "TAP receipt rejected");
                return Json(McpResponse::err(
                    id,
                    payment_required_error(json!({ "reason": e.to_string() })),
                ))
                .into_response();
            }

            tracing::debug!(%tool_name, value = signed_receipt.receipt.value, "TAP receipt accepted");
        }
    }

    // --- 3. Extract GraphQL query from arguments ---
    let arguments = params.get("arguments").unwrap_or(&serde_json::Value::Null);

    let query = match arguments.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return Json(McpResponse::err(
                id,
                invalid_params("missing 'query' in tool arguments"),
            ))
            .into_response();
        }
    };

    let variables = arguments
        .get("variables")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    // --- 4. Forward to graph-node ---
    let gn_base = subgraph.effective_graph_node_url(&state.config.graph_node.url);
    let endpoint = format!("{}/subgraphs/id/{}", gn_base, subgraph.deployment_id);

    let gn_response = state
        .http_client
        .post(&endpoint)
        .json(&json!({ "query": query, "variables": variables }))
        .send()
        .await;

    match gn_response {
        Err(e) => {
            tracing::error!(%tool_name, %endpoint, error = %e, "graph-node request failed");
            Json(McpResponse::err(id, internal_error(&format!("graph-node error: {e}"))))
                .into_response()
        }
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Err(e) => Json(McpResponse::err(
                    id,
                    internal_error(&format!("graph-node read error: {e}")),
                ))
                .into_response(),
                Ok(body) => {
                    if !status.is_success() {
                        tracing::warn!(%tool_name, %status, "graph-node returned non-2xx");
                    }
                    Json(McpResponse::ok(
                        id,
                        json!({
                            "content": [{ "type": "text", "text": body }],
                            "isError": !status.is_success()
                        }),
                    ))
                    .into_response()
                }
            }
        }
    }
}
