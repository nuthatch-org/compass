use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use compass_core::config::Config;
use compass_tap::domain_separator;

use crate::routes;
use crate::state::{AppState, ToolRoute};

pub async fn run(config: Config) -> Result<()> {
    let tap_domain_sep = domain_separator(
        &config.tap.eip712_domain_name,
        config.tap.eip712_chain_id,
        config.tap.eip712_verifying_contract,
    );

    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(config.graph_node.timeout_secs))
        .build()?;

    // ---- Introspect subgraphs -----------------------------------------------
    //
    // For each configured subgraph, try a GraphQL __schema introspection.
    // On success: generate one typed MCP tool per root query field.
    // On failure: fall back to the generic raw-query tool and log a warning.

    let mut tool_list: Vec<serde_json::Value> = Vec::new();
    let mut tool_routes: HashMap<String, ToolRoute> = HashMap::new();

    for subgraph in &config.subgraphs {
        let gn_url = subgraph.effective_graph_node_url(&config.graph_node.url);
        let price = subgraph.effective_price(config.tap.price_per_query_grt_wei);

        match compass_schema::introspect(&subgraph.deployment_id, gn_url, &http_client).await {
            Ok(fields) => {
                tracing::info!(
                    subgraph = %subgraph.name,
                    fields = fields.len(),
                    "introspected schema successfully"
                );
                for field in &fields {
                    let tool_json = compass_schema::field_to_mcp_tool(
                        &subgraph.name,
                        &subgraph.description,
                        &subgraph.network,
                        price,
                        &subgraph.deployment_id,
                        field,
                    );
                    let tool_name = format!("{}__{}", subgraph.name, field.name);
                    tool_routes.insert(
                        tool_name,
                        ToolRoute {
                            subgraph_name: subgraph.name.clone(),
                            field_name: field.name.clone(),
                            args: field.args.clone(),
                        },
                    );
                    tool_list.push(tool_json);
                }
            }
            Err(e) => {
                tracing::warn!(
                    subgraph = %subgraph.name,
                    error = %e,
                    "schema introspection failed — falling back to generic tool"
                );
                let tool = compass_schema::generic_tool(
                    &subgraph.name,
                    &subgraph.description,
                    &subgraph.network,
                );
                let annotated = format!(
                    "{}\n\nPrice: {} GRT wei | deployment: {} | network: {}",
                    tool.description, price, subgraph.deployment_id, subgraph.network
                );
                tool_list.push(serde_json::json!({
                    "name": tool.name,
                    "description": annotated,
                    "inputSchema": tool.input_schema,
                }));
                // Generic tools are routed by subgraph name only — no entry in tool_routes.
            }
        }
    }

    tracing::info!(
        total_tools = tool_list.len(),
        typed_tools = tool_routes.len(),
        "tool registry built"
    );

    // ---- Build state and start server ---------------------------------------

    let state = AppState {
        config: Arc::new(config.clone()),
        http_client,
        tap_domain_sep,
        tool_list: Arc::new(tool_list),
        tool_routes: Arc::new(tool_routes),
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
