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
use compass_tap::verify_receipt;

use crate::state::AppState;

/// TAP receipt HTTP header name (lowercase; axum normalises on lookup).
const TAP_RECEIPT_HEADER: &str = "tap-receipt";
/// x402 payment proof header name.
const X_PAYMENT_HEADER: &str = "x-payment";

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
        "ping" => handle_ping(req.id).into_response(),
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
            "capabilities": { "tools": {} },
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
    Json(McpResponse::ok(id, json!({ "tools": *state.tool_list })))
}

// ---- tools/call ------------------------------------------------------------

async fn handle_tools_call(
    state: &AppState,
    id: Option<serde_json::Value>,
    params: &serde_json::Value,
    headers: &HeaderMap,
) -> Response {
    // --- 1. Resolve tool name ------------------------------------------------
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return Json(McpResponse::err(id, invalid_params("missing 'name' in params")))
                .into_response();
        }
    };

    // --- 2. Route to subgraph ------------------------------------------------
    //
    // Introspected tools: name is `{subgraph}__{field}` — found in tool_routes.
    // Generic tools: name matches SubgraphConfig.name directly.

    let (subgraph, query, variables) =
        match resolve_tool(state, tool_name, params.get("arguments").unwrap_or(&json!(null))) {
            Ok(t) => t,
            Err(e) => {
                return Json(McpResponse::err(id, invalid_params(&e))).into_response();
            }
        };

    let price = subgraph.effective_price(state.config.tap.price_per_query_grt_wei);
    let indexer_addr = state.config.gateway.indexer_address.to_string();

    // --- 3. Check payment: TAP receipt (primary) or x402 X-Payment ----------
    match (headers.get(TAP_RECEIPT_HEADER), headers.get(X_PAYMENT_HEADER)) {
        (None, None) => {
            let body = compass_x402::payment_required_body(tool_name, price, &indexer_addr);
            return (StatusCode::PAYMENT_REQUIRED, Json(body)).into_response();
        }

        (Some(raw), _) => {
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

            let signed_receipt: compass_tap::SignedReceipt = match serde_json::from_str(header_str)
            {
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

        (None, Some(raw)) => {
            let x402_cfg = match &state.config.x402 {
                Some(cfg) => cfg,
                None => {
                    return Json(McpResponse::err(
                        id,
                        invalid_params("x402 payments not configured on this gateway"),
                    ))
                    .into_response();
                }
            };

            let header_str = match raw.to_str() {
                Ok(s) => s,
                Err(_) => {
                    return Json(McpResponse::err(
                        id,
                        invalid_params("x-payment header is not valid UTF-8"),
                    ))
                    .into_response();
                }
            };

            let required_usdc = compass_x402::usdc_micro_for(price);
            if let Err(e) = compass_x402::validate_x_payment(
                header_str,
                required_usdc,
                &indexer_addr,
                &x402_cfg.base_rpc_url,
                &state.http_client,
            )
            .await
            {
                tracing::warn!(%tool_name, error = %e, "x402 payment rejected");
                return Json(McpResponse::err(
                    id,
                    payment_required_error(json!({ "reason": e.to_string() })),
                ))
                .into_response();
            }

            tracing::debug!(%tool_name, required_usdc, "x402 payment accepted");
        }
    }

    // --- 4. Forward to graph-node --------------------------------------------
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

// ---- Tool resolution --------------------------------------------------------

/// Resolve a tool name to `(subgraph_config, graphql_query, variables)`.
///
/// - Tool names containing `__` are introspected tools: the query is auto-built.
/// - All other names are generic tools: the agent supplies a raw `query` arg.
fn resolve_tool<'a>(
    state: &'a AppState,
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Result<(&'a compass_core::config::SubgraphConfig, String, serde_json::Value), String> {
    // Introspected tool path.
    if let Some(route) = state.tool_routes.get(tool_name) {
        let subgraph = state
            .config
            .subgraphs
            .iter()
            .find(|s| s.name == route.subgraph_name)
            .ok_or_else(|| format!("internal: subgraph '{}' not found", route.subgraph_name))?;

        let (query, vars) =
            compass_schema::build_graphql_query(&route.field_name, arguments, &route.args);
        return Ok((subgraph, query, vars));
    }

    // Generic tool path: look up subgraph by plain name.
    let subgraph = state
        .config
        .subgraphs
        .iter()
        .find(|s| s.name == tool_name)
        .ok_or_else(|| format!("unknown tool: {tool_name}"))?;

    let query = arguments
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'query' in tool arguments".to_string())?
        .to_string();

    let variables = arguments
        .get("variables")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    Ok((subgraph, query, variables))
}
