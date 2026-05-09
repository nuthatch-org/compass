//! GraphQL schema introspection → MCP tool schema derivation.
//!
//! Two modes per subgraph:
//!   - **Generic** (fallback): one tool accepting raw `query` + `variables`.
//!   - **Introspected**: one tool per root query field with typed args.
//!     Tool names are `{subgraph_name}__{field_name}`.
//!
//! Agents using introspected tools pass typed args; the gateway auto-constructs
//! the GraphQL query. A `_fields` arg controls the selection set (default: `id`).
//! A `_query` arg provides a raw GraphQL override for complex queries.

use anyhow::Context;
use serde::Deserialize;

use compass_core::types::McpTool;

// ---- Public types -----------------------------------------------------------

/// Metadata for a single GraphQL query-field argument, used both for building
/// the MCP input schema and for auto-constructing GraphQL at call time.
#[derive(Debug, Clone)]
pub struct FieldArg {
    pub name: String,
    pub description: Option<String>,
    /// JSON Schema fragment for this argument (e.g. `{"type":"string"}`).
    pub json_schema: serde_json::Value,
    /// True if the base GraphQL type is ENUM — values are not quoted in GraphQL.
    pub is_enum: bool,
    /// True if the arg is NON_NULL with no default value.
    pub required: bool,
}

/// A root-level query field extracted from a subgraph's GraphQL schema.
#[derive(Debug, Clone)]
pub struct IntrospectedField {
    pub name: String,
    pub description: Option<String>,
    pub args: Vec<FieldArg>,
}

// ---- Internal introspection deserialization types ---------------------------

#[derive(Deserialize)]
struct IntrospectionResp {
    data: IntrospectionData,
}

#[derive(Deserialize)]
struct IntrospectionData {
    #[serde(rename = "__schema")]
    schema: SchemaData,
}

#[derive(Deserialize)]
struct SchemaData {
    #[serde(rename = "queryType")]
    query_type: QueryTypeData,
}

#[derive(Deserialize)]
struct QueryTypeData {
    fields: Vec<GqlField>,
}

#[derive(Deserialize)]
struct GqlField {
    name: String,
    description: Option<String>,
    args: Vec<GqlArg>,
}

#[derive(Deserialize)]
struct GqlArg {
    name: String,
    description: Option<String>,
    #[serde(rename = "type")]
    ty: GqlType,
    #[serde(rename = "defaultValue")]
    default_value: Option<String>,
}

#[derive(Deserialize)]
struct GqlType {
    kind: String,
    name: Option<String>,
    #[serde(rename = "ofType")]
    of_type: Option<Box<GqlType>>,
}

// ---- Introspection ----------------------------------------------------------

/// Introspection query — 4 levels of ofType to handle NON_NULL(LIST(NON_NULL(T))).
const INTROSPECTION_QUERY: &str = r#"{
  __schema {
    queryType {
      fields(includeDeprecated: false) {
        name
        description
        args {
          name
          description
          type { kind name ofType { kind name ofType { kind name ofType { kind name } } } }
          defaultValue
        }
      }
    }
  }
}"#;

/// Fetch and parse the root query fields of a deployed subgraph.
///
/// Returns `Err` if graph-node is unreachable or the subgraph isn't synced yet.
/// The caller should fall back to `generic_tool` on error.
pub async fn introspect(
    deployment_id: &str,
    graph_node_url: &str,
    client: &reqwest::Client,
) -> anyhow::Result<Vec<IntrospectedField>> {
    let url = format!("{}/subgraphs/id/{}", graph_node_url, deployment_id);

    let raw: serde_json::Value = client
        .post(&url)
        .json(&serde_json::json!({ "query": INTROSPECTION_QUERY }))
        .send()
        .await
        .context("introspection HTTP request failed")?
        .json()
        .await
        .context("introspection response is not valid JSON")?;

    if let Some(errors) = raw.get("errors").filter(|e| !e.is_null()) {
        anyhow::bail!("GraphQL errors during introspection: {errors}");
    }

    let typed: IntrospectionResp =
        serde_json::from_value(raw).context("unexpected introspection response shape")?;

    let fields = typed
        .data
        .schema
        .query_type
        .fields
        .into_iter()
        .map(|f| {
            let args = f
                .args
                .into_iter()
                .map(|a| {
                    let (json_schema, non_null, is_enum) = gql_type_to_json_schema(&a.ty);
                    FieldArg {
                        name: a.name,
                        description: a.description,
                        json_schema,
                        is_enum,
                        required: non_null && a.default_value.is_none(),
                    }
                })
                .collect();
            IntrospectedField { name: f.name, description: f.description, args }
        })
        .collect();

    Ok(fields)
}

// ---- Tool builders ----------------------------------------------------------

/// Generic MCP tool for a subgraph — fallback when introspection is unavailable.
///
/// Exposes one tool accepting any raw GraphQL `query` + optional `variables`.
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

/// Generate a typed MCP tool JSON for a single introspected query field.
///
/// Tool name: `{subgraph_name}__{field_name}`.
/// Args are derived from the GraphQL field's argument types.
/// Also adds `_fields` (selection set, default `id`) and `_query` (raw override).
pub fn field_to_mcp_tool(
    subgraph_name: &str,
    subgraph_description: &str,
    subgraph_network: &str,
    price_grt_wei: u64,
    deployment_id: &str,
    field: &IntrospectedField,
) -> serde_json::Value {
    let tool_name = format!("{}__{}", subgraph_name, field.name);

    let description = format!(
        "{} — `{}` query\n\nPrice: {} GRT wei | deployment: {} | network: {}",
        subgraph_description, field.name, price_grt_wei, deployment_id, subgraph_network,
    );

    let mut properties = serde_json::Map::new();
    let mut required_names: Vec<serde_json::Value> = Vec::new();

    for arg in &field.args {
        let mut schema = arg.json_schema.clone();
        if let (Some(desc), Some(obj)) = (&arg.description, schema.as_object_mut()) {
            obj.insert("description".to_string(), serde_json::Value::String(desc.clone()));
        }
        properties.insert(arg.name.clone(), schema);
        if arg.required {
            required_names.push(serde_json::Value::String(arg.name.clone()));
        }
    }

    properties.insert(
        "_fields".to_string(),
        serde_json::json!({
            "type": "string",
            "description": "Space-separated entity fields to return (default: id). Example: \"id symbol decimals totalSupply\""
        }),
    );
    properties.insert(
        "_query".to_string(),
        serde_json::json!({
            "type": "string",
            "description": "Raw GraphQL override for complex queries — bypasses typed arg auto-construction"
        }),
    );
    properties.insert(
        "_variables".to_string(),
        serde_json::json!({
            "type": "object",
            "description": "GraphQL variables to pass alongside a _query raw override"
        }),
    );

    let mut input_schema = serde_json::json!({
        "type": "object",
        "properties": properties,
    });
    if !required_names.is_empty() {
        input_schema["required"] = serde_json::Value::Array(required_names);
    }

    serde_json::json!({
        "name": tool_name,
        "description": description,
        "inputSchema": input_schema,
    })
}

// ---- Auto query construction ------------------------------------------------

/// Build a GraphQL query string from typed tool arguments.
///
/// - `_query` arg: returned as-is (raw override).
/// - `_fields` arg: controls the selection set (default: `id`).
/// - `_variables` arg: ignored here (only relevant alongside `_query`).
/// - All other args become the field's argument list in the query.
pub fn build_graphql_query(
    field_name: &str,
    args: &serde_json::Value,
    field_args: &[FieldArg],
) -> (String, serde_json::Value) {
    let obj = match args.as_object() {
        Some(o) => o,
        None => {
            return (
                format!("{{ {field_name} {{ id }} }}"),
                serde_json::Value::Null,
            )
        }
    };

    // Raw override path.
    if let Some(raw_query) = obj.get("_query").and_then(|q| q.as_str()) {
        let vars = obj
            .get("_variables")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        return (raw_query.to_string(), vars);
    }

    let selection = obj.get("_fields").and_then(|f| f.as_str()).unwrap_or("id");

    let gql_args: Vec<String> = obj
        .iter()
        .filter(|(k, _)| !k.starts_with('_'))
        .map(|(k, v)| {
            let is_enum =
                field_args.iter().find(|a| &a.name == k).map(|a| a.is_enum).unwrap_or(false);
            format!("{k}: {}", to_graphql_literal(v, is_enum))
        })
        .collect();

    let query = if gql_args.is_empty() {
        format!("{{ {field_name} {{ {selection} }} }}")
    } else {
        format!("{{ {field_name}({}) {{ {selection} }} }}", gql_args.join(", "))
    };

    (query, serde_json::Value::Null)
}

/// Render a JSON value as a GraphQL literal.
///
/// Enum values are not quoted; strings are double-quoted with `"` escaped.
/// Input objects use `{key: val}` syntax; arrays use `[val]`.
fn to_graphql_literal(v: &serde_json::Value, is_enum: bool) -> String {
    match v {
        serde_json::Value::String(s) if is_enum => s.clone(),
        serde_json::Value::String(s) => format!("\"{}\"", s.replace('"', "\\\"")),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Object(o) => {
            let fields: Vec<_> =
                o.iter().map(|(k, v)| format!("{k}: {}", to_graphql_literal(v, false))).collect();
            format!("{{{}}}", fields.join(", "))
        }
        serde_json::Value::Array(a) => {
            let items: Vec<_> = a.iter().map(|v| to_graphql_literal(v, false)).collect();
            format!("[{}]", items.join(", "))
        }
    }
}

// ---- Type conversion --------------------------------------------------------

/// Convert a GraphQL type reference to a JSON Schema fragment.
///
/// Returns `(json_schema, is_non_null, is_enum)`.
fn gql_type_to_json_schema(ty: &GqlType) -> (serde_json::Value, bool, bool) {
    match ty.kind.as_str() {
        "NON_NULL" => match ty.of_type.as_deref() {
            None => (serde_json::json!({"type": "string"}), true, false),
            Some(inner) => {
                let (schema, _, is_enum) = gql_type_to_json_schema(inner);
                (schema, true, is_enum)
            }
        },
        "LIST" => match ty.of_type.as_deref() {
            None => (serde_json::json!({"type": "array", "items": {"type": "string"}}), false, false),
            Some(inner) => {
                let (items, _, is_enum) = gql_type_to_json_schema(inner);
                (serde_json::json!({"type": "array", "items": items}), false, is_enum)
            }
        },
        "ENUM" => {
            let name = ty.name.as_deref().unwrap_or("Enum");
            (serde_json::json!({"type": "string", "description": format!("Enum type: {name}")}), false, true)
        }
        "INPUT_OBJECT" => {
            let name = ty.name.as_deref().unwrap_or("Object");
            (serde_json::json!({"type": "object", "description": format!("Input type: {name}")}), false, false)
        }
        _ => {
            // SCALAR (and anything unrecognised)
            let schema = match ty.name.as_deref().unwrap_or("String") {
                "Int" => serde_json::json!({"type": "integer"}),
                "Float" => serde_json::json!({"type": "number"}),
                "Boolean" => serde_json::json!({"type": "boolean"}),
                _ => serde_json::json!({"type": "string"}),
            };
            (schema, false, false)
        }
    }
}
