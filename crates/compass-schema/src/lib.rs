//! GraphQL schema introspection → MCP tool schema derivation.
//!
//! Week 3 (current): `generic_tool` returns one tool per subgraph, accepting
//! raw `query` + optional `variables`. Agents can run any GraphQL query.
//!
//! Week 6 (TODO): `introspect` fetches `__schema` from graph-node, then
//! `schema_to_tools` generates one MCP tool per root query field with a
//! JSON Schema derived from the field's argument types. This gives agents
//! strongly-typed, auto-documented access to each entity without needing to
//! know GraphQL.

use compass_core::types::McpTool;

/// Generate the generic MCP tool for a subgraph (week 3 implementation).
///
/// Exposes a single tool that accepts any raw GraphQL `query` string plus
/// optional `variables`. Week 6 will replace this with per-entity tools.
pub fn generic_tool(name: &str, description: &str, network: &str) -> McpTool {
    McpTool {
        name: name.to_string(),
        description: format!("{description} (network: {network})"),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "GraphQL query string (e.g. '{ tokens(first: 5) { id symbol } }')"
                },
                "variables": {
                    "type": "object",
                    "description": "GraphQL query variables as a JSON object (optional)"
                }
            },
            "required": ["query"]
        }),
    }
}

// TODO week 6:
//
// pub async fn introspect(deployment_id: &str, graph_node_url: &str, client: &reqwest::Client)
//     -> anyhow::Result<Vec<McpTool>>
//
// 1. POST {graph_node_url}/subgraphs/id/{deployment_id} with __schema introspection query
// 2. Walk queryType fields → one tool per root query field
// 3. Walk each field's args → JSON Schema properties
// 4. Cache result with TTL (invalidate on deployment update)
