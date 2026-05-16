//! Render a `GraphqlSelect` into `(query_doc, variables)` for HTTP POST.
//!
//! The filter argument is rendered **inline** (as GraphQL value syntax)
//! to avoid having to know the server's input-object type names. Pagination
//! goes through typed variables (`$limit: Int`, `$offset: Int`) which the
//! GraphQL spec covers without any schema lookup.
//!
//! That trade-off makes the renderer schema-agnostic for v1; Phase 5
//! will wire a schema map that can promote the filter to a typed
//! variable when the input-type name is known.

use serde_json::{Map, Value};
use vantage_core::{Result, error};
use vantage_expressions::Order;

use crate::graphql::condition::{FilterDialect, GraphqlCondition};
use crate::graphql::select::GraphqlSelect;

/// The rendered output of [`GraphqlSelect::render`] — what
/// `GraphqlApi::post_graphql` consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedQuery {
    pub query: String,
    pub variables: Map<String, Value>,
}

impl GraphqlSelect {
    /// Produce a `(query, variables)` pair. Async because conditions
    /// may carry `Deferred` branches that resolve at fetch time.
    pub async fn render(&self) -> Result<RenderedQuery> {
        let root = self
            .root_field
            .as_deref()
            .ok_or_else(|| error!("GraphqlSelect: root_field is required"))?;

        let mut variables = Map::new();
        let mut var_decls: Vec<String> = Vec::new();
        let mut args: Vec<String> = Vec::new();

        // ── Filter (inline) ──────────────────────────────────────
        if !self.conditions.is_empty() {
            let combined = if self.conditions.len() == 1 {
                self.conditions[0].render(self.dialect).await?
            } else {
                GraphqlCondition::And(self.conditions.clone())
                    .render(self.dialect)
                    .await?
            };
            let arg_name = self
                .filter_arg_name
                .as_deref()
                .unwrap_or(match self.dialect {
                    FilterDialect::Hasura => "where",
                    FilterDialect::Generic => "find",
                });
            args.push(format!(
                "{}: {}",
                arg_name,
                json_to_graphql_value(&combined)
            ));
        }

        // ── Order (Hasura only for now) ──────────────────────────
        if !self.sort.is_empty() && matches!(self.dialect, FilterDialect::Hasura) {
            let entries: Vec<String> = self
                .sort
                .iter()
                .map(|(field, order)| format!("{}: {}", field, render_order(*order)))
                .collect();
            args.push(format!("order_by: [{{{}}}]", entries.join(", ")));
        }

        // ── Pagination via variables ─────────────────────────────
        if let Some(limit) = self.limit {
            variables.insert("limit".into(), Value::Number(limit.into()));
            var_decls.push("$limit: Int".into());
            args.push("limit: $limit".into());
        }
        if let Some(skip) = self.skip {
            variables.insert("offset".into(), Value::Number(skip.into()));
            var_decls.push("$offset: Int".into());
            args.push("offset: $offset".into());
        }

        // ── Selection set ────────────────────────────────────────
        let selection_set = render_selection_set(self).await?;

        // ── Assemble document ────────────────────────────────────
        let op_name = self.operation_name.as_deref().unwrap_or("");
        let op_decls = if var_decls.is_empty() {
            String::new()
        } else {
            format!("({})", var_decls.join(", "))
        };
        let args_str = if args.is_empty() {
            String::new()
        } else {
            format!("({})", args.join(", "))
        };

        let query = if op_name.is_empty() && op_decls.is_empty() {
            format!("query {{ {}{} {} }}", root, args_str, selection_set)
        } else {
            format!(
                "query {}{} {{ {}{} {} }}",
                op_name, op_decls, root, args_str, selection_set
            )
        };

        Ok(RenderedQuery { query, variables })
    }

    /// Human-readable preview — same shape as `render()` but synchronous
    /// and lossy (skips Deferred resolution). Useful for logs and tests.
    pub fn preview(&self) -> String {
        let root = self.root_field.as_deref().unwrap_or("?");
        let count = self.conditions.len();
        let limit = self
            .limit
            .map(|l| format!(", limit: {}", l))
            .unwrap_or_default();
        let skip = self
            .skip
            .map(|s| format!(", offset: {}", s))
            .unwrap_or_default();
        let where_part = if count > 0 {
            format!("(<{} conditions>{}{})", count, limit, skip)
        } else if !limit.is_empty() || !skip.is_empty() {
            format!(
                "({})",
                &format!("{}{}", limit, skip).trim_start_matches(", ")
            )
        } else {
            String::new()
        };
        let selection = if self.fields.is_empty() {
            "{ ... }".to_string()
        } else {
            format!("{{ {} }}", self.fields.join(" "))
        };
        format!("{}{} {}", root, where_part, selection)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Render a selection set as `{ field1 field2 nested { ... } }`.
fn render_selection_set<'a>(
    select: &'a GraphqlSelect,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        if select.fields.is_empty() && select.sub_selections.is_empty() {
            return Err(error!(
                "GraphqlSelect: selection set is empty",
                root = select.root_field.clone().unwrap_or_default()
            ));
        }
        let mut parts: Vec<String> = select.fields.clone();
        for (field, child) in &select.sub_selections {
            let inner = render_inline_subselection(child).await?;
            parts.push(format!("{}{}", field, inner));
        }
        Ok(format!("{{ {} }}", parts.join(" ")))
    })
}

/// Render a sub-selection (a child of a parent's selection set). Args
/// are rendered inline (no variables) since variable scope is tied to
/// the operation, not the sub-field.
fn render_inline_subselection<'a>(
    child: &'a GraphqlSelect,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'a>> {
    Box::pin(async move {
        let mut args: Vec<String> = Vec::new();
        if !child.conditions.is_empty() {
            let condition = if child.conditions.len() == 1 {
                child.conditions[0].clone()
            } else {
                GraphqlCondition::And(child.conditions.clone())
            };
            let rendered = condition.render(child.dialect).await?;
            let arg_name = child
                .filter_arg_name
                .as_deref()
                .unwrap_or(match child.dialect {
                    FilterDialect::Hasura => "where",
                    FilterDialect::Generic => "find",
                });
            args.push(format!(
                "{}: {}",
                arg_name,
                json_to_graphql_value(&rendered)
            ));
        }
        if let Some(limit) = child.limit {
            args.push(format!("limit: {}", limit));
        }
        if let Some(skip) = child.skip {
            args.push(format!("offset: {}", skip));
        }
        let args_str = if args.is_empty() {
            String::new()
        } else {
            format!("({})", args.join(", "))
        };
        let inner = render_selection_set(child).await?;
        Ok(format!("{} {}", args_str, inner))
    })
}

/// Render a `serde_json::Value` as a GraphQL value (object keys are
/// unquoted, strings get escaped).
pub(crate) fn json_to_graphql_value(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", escape_string(s)),
        Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(json_to_graphql_value).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(obj) => {
            let parts: Vec<String> = obj
                .iter()
                .map(|(k, v)| format!("{}: {}", k, json_to_graphql_value(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn render_order(order: Order) -> &'static str {
    if order.ascending { "asc" } else { "desc" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use crate::graphql::condition::{FieldCondition, FilterDialect, GraphqlOp};

    #[tokio::test]
    async fn renders_minimal_query() {
        let q = GraphqlSelect::new()
            .with_root_field("launches")
            .with_field("id")
            .with_field("mission_name")
            .render()
            .await
            .unwrap();
        assert_eq!(q.query, "query { launches { id mission_name } }");
        assert!(q.variables.is_empty());
    }

    #[tokio::test]
    async fn renders_generic_filter_inline() {
        let q = GraphqlSelect::new()
            .with_root_field("launches")
            .with_field("id")
            .with_dialect(FilterDialect::Generic)
            .with_condition(GraphqlCondition::Field(FieldCondition::new(
                "mission_name",
                GraphqlOp::Eq,
                json!("FalconSat"),
            )))
            .render()
            .await
            .unwrap();
        assert_eq!(
            q.query,
            "query { launches(find: {mission_name: \"FalconSat\"}) { id } }"
        );
        assert!(q.variables.is_empty());
    }

    #[tokio::test]
    async fn renders_hasura_filter_inline() {
        let q = GraphqlSelect::new()
            .with_root_field("users")
            .with_field("id")
            .with_dialect(FilterDialect::Hasura)
            .with_condition(GraphqlCondition::Field(FieldCondition::new(
                "age",
                GraphqlOp::Gt,
                json!(30),
            )))
            .render()
            .await
            .unwrap();
        assert_eq!(q.query, "query { users(where: {age: {_gt: 30}}) { id } }");
    }

    #[tokio::test]
    async fn renders_pagination_as_variables() {
        let q = GraphqlSelect::new()
            .with_root_field("launches")
            .with_field("id")
            .with_limit(Some(10), Some(20))
            .render()
            .await
            .unwrap();
        assert_eq!(
            q.query,
            "query ($limit: Int, $offset: Int) { launches(limit: $limit, offset: $offset) { id } }"
        );
        assert_eq!(q.variables.get("limit"), Some(&json!(10)));
        assert_eq!(q.variables.get("offset"), Some(&json!(20)));
    }

    #[tokio::test]
    async fn renders_with_operation_name() {
        let q = GraphqlSelect::new()
            .with_root_field("launches")
            .with_operation_name("GetLaunches")
            .with_field("id")
            .with_limit(Some(5), None)
            .render()
            .await
            .unwrap();
        assert_eq!(
            q.query,
            "query GetLaunches($limit: Int) { launches(limit: $limit) { id } }"
        );
    }

    #[tokio::test]
    async fn renders_sub_selection() {
        let rocket = GraphqlSelect::new().with_field("id").with_field("name");
        let q = GraphqlSelect::new()
            .with_root_field("launches")
            .with_field("id")
            .with_field("mission_name")
            .with_sub_selection("rocket", rocket)
            .render()
            .await
            .unwrap();
        assert_eq!(
            q.query,
            "query { launches { id mission_name rocket { id name } } }"
        );
    }

    #[tokio::test]
    async fn renders_hasura_order_by() {
        let q = GraphqlSelect::new()
            .with_root_field("users")
            .with_field("id")
            .with_dialect(FilterDialect::Hasura)
            .with_order("created_at", Order::Desc)
            .render()
            .await
            .unwrap();
        assert_eq!(
            q.query,
            "query { users(order_by: [{created_at: desc}]) { id } }"
        );
    }

    #[tokio::test]
    async fn empty_selection_set_errors() {
        let err = GraphqlSelect::new()
            .with_root_field("launches")
            .render()
            .await
            .unwrap_err();
        assert!(err.to_string().contains("selection set"));
    }

    #[tokio::test]
    async fn missing_root_field_errors() {
        let err = GraphqlSelect::new()
            .with_field("id")
            .render()
            .await
            .unwrap_err();
        assert!(err.to_string().contains("root_field"));
    }

    #[test]
    fn json_to_graphql_value_strips_string_quotes_in_keys() {
        let v = json!({ "mission_name": "FalconSat", "year": 2006 });
        let rendered = json_to_graphql_value(&v);
        assert_eq!(rendered, "{mission_name: \"FalconSat\", year: 2006}");
    }
}
