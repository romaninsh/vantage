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

        // ── Literal root arguments ───────────────────────────────
        // First, so a mandatory `input:` reads before the filter it
        // often has nothing to do with.
        if let Some(Value::Object(map)) = &self.root_args {
            for (name, value) in map {
                args.push(format!("{}: {}", name, json_to_graphql_value(value)));
            }
        }

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
        // Wrapped in the response envelope, so the query asks for rows
        // exactly where the decoder will look for them.
        let mut selection_set = render_selection_set(self).await?;
        for segment in self.response_path.iter().rev() {
            selection_set = format!("{{ {} {} }}", segment, selection_set);
        }

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

    /// The document [`render`](Self::render) would produce, built synchronously
    /// so it can be shown without sending anything.
    ///
    /// Mirrors `render` arm for arm — same filter, same order, same selection
    /// set, same response envelope — and differs in exactly two ways, both
    /// forced by not being allowed to do any work:
    ///
    /// 1. A **deferred** condition renders as a `**deferred(...)` marker rather
    ///    than being awaited. Resolving one means asking the caller for a value
    ///    that only exists mid-fetch.
    /// 2. Pagination is **inlined** (`limit: 5`) instead of going through the
    ///    `$limit` variable, so the document reads standalone. The server is
    ///    told the same numbers either way.
    ///
    /// Everything else is the query that gets sent, and pastes into a GraphQL
    /// client as-is.
    pub fn preview(&self) -> String {
        let root = self.root_field.as_deref().unwrap_or("<no root_field>");
        let mut args: Vec<String> = Vec::new();

        if let Some(Value::Object(map)) = &self.root_args {
            for (name, value) in map {
                args.push(format!("{}: {}", name, json_to_graphql_value(value)));
            }
        }

        if !self.conditions.is_empty() {
            let combined = if self.conditions.len() == 1 {
                self.conditions[0].render_preview(self.dialect)
            } else {
                GraphqlCondition::And(self.conditions.clone()).render_preview(self.dialect)
            };
            let arg_name = self
                .filter_arg_name
                .as_deref()
                .unwrap_or(match self.dialect {
                    FilterDialect::Hasura => "where",
                    FilterDialect::Generic => "find",
                });
            match combined {
                Ok(value) => args.push(format!("{}: {}", arg_name, json_to_graphql_value(&value))),
                // A dialect that cannot spell this filter fails at send time
                // too. Surfacing that here is the point of previewing.
                Err(e) => args.push(format!("{}: <unrenderable: {}>", arg_name, e)),
            }
        }

        if !self.sort.is_empty() && matches!(self.dialect, FilterDialect::Hasura) {
            let entries: Vec<String> = self
                .sort
                .iter()
                .map(|(field, order)| format!("{}: {}", field, render_order(*order)))
                .collect();
            args.push(format!("order_by: [{{{}}}]", entries.join(", ")));
        }

        if let Some(limit) = self.limit {
            args.push(format!("limit: {}", limit));
        }
        if let Some(skip) = self.skip {
            args.push(format!("offset: {}", skip));
        }

        let mut selection_set = preview_selection_set(self);
        for segment in self.response_path.iter().rev() {
            selection_set = format!("{{ {} {} }}", segment, selection_set);
        }

        let args_str = if args.is_empty() {
            String::new()
        } else {
            format!("({})", args.join(", "))
        };
        let op_name = self.operation_name.as_deref().unwrap_or("");
        if op_name.is_empty() {
            format!("query {{ {}{} {} }}", root, args_str, selection_set)
        } else {
            format!(
                "query {} {{ {}{} {} }}",
                op_name, root, args_str, selection_set
            )
        }
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
        let mut parts = FieldTree::from_paths(&select.fields).into_parts();
        for (field, child) in &select.sub_selections {
            let inner = render_inline_subselection(child).await?;
            parts.push(format!("{}{}", field, inner));
        }
        Ok(format!("{{ {} }}", parts.join(" ")))
    })
}

/// Synchronous [`render_selection_set`], for [`GraphqlSelect::preview`].
///
/// Where the async version errors on an empty selection set, this reports it
/// inline as `{ <empty selection set> }` — a preview's job is to show the query
/// as it stands, and "you selected no fields" is exactly the fault worth seeing.
fn preview_selection_set(select: &GraphqlSelect) -> String {
    if select.fields.is_empty() && select.sub_selections.is_empty() {
        return "{ <empty selection set> }".to_string();
    }
    let mut parts = FieldTree::from_paths(&select.fields).into_parts();
    for (field, child) in &select.sub_selections {
        parts.push(format!("{}{}", field, preview_inline_subselection(child)));
    }
    format!("{{ {} }}", parts.join(" "))
}

/// Synchronous [`render_inline_subselection`], for [`GraphqlSelect::preview`].
fn preview_inline_subselection(child: &GraphqlSelect) -> String {
    let mut args: Vec<String> = Vec::new();
    if !child.conditions.is_empty() {
        let condition = if child.conditions.len() == 1 {
            child.conditions[0].clone()
        } else {
            GraphqlCondition::And(child.conditions.clone())
        };
        let arg_name = child
            .filter_arg_name
            .as_deref()
            .unwrap_or(match child.dialect {
                FilterDialect::Hasura => "where",
                FilterDialect::Generic => "find",
            });
        match condition.render_preview(child.dialect) {
            Ok(value) => args.push(format!("{}: {}", arg_name, json_to_graphql_value(&value))),
            Err(e) => args.push(format!("{}: <unrenderable: {}>", arg_name, e)),
        }
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
    format!("{} {}", args_str, preview_selection_set(child))
}

/// Dotted field paths grouped back into the tree GraphQL wants:
/// `["run.id", "run.state", "isModule"]` → `run { id state } isModule`.
///
/// Insertion order is preserved at every level, so the rendered query
/// reads in the order the columns were declared. A path that is both a
/// leaf and a parent (`run` alongside `run.id`) renders as the parent —
/// selecting an object without a sub-selection is invalid GraphQL anyway.
#[derive(Default)]
struct FieldTree {
    children: Vec<(String, FieldTree)>,
}

impl FieldTree {
    fn from_paths<S: AsRef<str>>(paths: &[S]) -> Self {
        let mut root = FieldTree::default();
        for path in paths {
            root.insert(path.as_ref());
        }
        root
    }

    fn insert(&mut self, path: &str) {
        let (head, rest) = match path.split_once('.') {
            Some((head, rest)) => (head, Some(rest)),
            None => (path, None),
        };
        let at = match self.children.iter().position(|(name, _)| name == head) {
            Some(at) => at,
            None => {
                self.children.push((head.to_string(), FieldTree::default()));
                self.children.len() - 1
            }
        };
        if let Some(rest) = rest {
            self.children[at].1.insert(rest);
        }
    }

    fn into_parts(self) -> Vec<String> {
        self.children
            .into_iter()
            .map(|(name, child)| {
                if child.children.is_empty() {
                    name
                } else {
                    format!("{} {{ {} }}", name, child.into_parts().join(" "))
                }
            })
            .collect()
    }
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
    use crate::graphql::types::AnyGraphqlType;
    use vantage_expressions::{DeferredFn, ExpressiveEnum};

    /// The guarantee worth pinning: for a query with no deferred conditions and
    /// no pagination, previewing and rendering produce the *same document*. If
    /// these ever diverge, a preview has started lying about what gets sent.
    #[tokio::test]
    async fn preview_matches_render_for_a_literal_query() {
        let select = GraphqlSelect::new()
            .with_root_field("launches")
            .with_field("id")
            .with_field("rocket.name")
            .with_dialect(FilterDialect::Generic)
            .with_condition(GraphqlCondition::Field(FieldCondition::new(
                "mission_name",
                GraphqlOp::Eq,
                json!("FalconSat"),
            )));

        let rendered = select.render().await.unwrap();
        assert_eq!(select.preview(), rendered.query);
        // And it is the real thing, not two matching placeholders.
        assert!(
            select.preview().contains("\"FalconSat\""),
            "the condition value is rendered, not summarised: {}",
            select.preview()
        );
    }

    /// Pagination is the one deliberate difference: `render` sends `$limit` as a
    /// variable, `preview` inlines the number so the document reads standalone.
    #[test]
    fn preview_inlines_pagination() {
        let preview = GraphqlSelect::new()
            .with_root_field("launches")
            .with_field("id")
            .with_limit(Some(5), Some(10))
            .preview();
        assert_eq!(preview, "query { launches(limit: 5, offset: 10) { id } }");
    }

    /// A deferred condition is the other: its value only exists mid-fetch, so
    /// preview marks it instead of awaiting it.
    #[test]
    fn preview_marks_a_deferred_condition_rather_than_resolving_it() {
        let preview = GraphqlSelect::new()
            .with_root_field("runs")
            .with_field("id")
            .with_dialect(FilterDialect::Generic)
            .with_condition(GraphqlCondition::DeferredField {
                field: "stack_id".into(),
                op: GraphqlOp::Eq,
                value_fn: DeferredFn::new(|| {
                    Box::pin(async { Ok(ExpressiveEnum::Scalar(AnyGraphqlType::from(json!("x")))) })
                }),
            })
            .preview();
        assert!(
            preview.contains("**deferred(stack_id"),
            "deferred values are marked, never awaited: {preview}"
        );
    }

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

#[cfg(test)]
mod nesting_tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn dotted_fields_group_into_nested_selections() {
        let q = GraphqlSelect::new()
            .with_root_field("searchRuns")
            .with_field("run.id")
            .with_field("run.state")
            .with_field("run.commit.hash")
            .with_field("stack.id")
            .with_field("isModule")
            .render()
            .await
            .unwrap();
        assert_eq!(
            q.query,
            "query { searchRuns { run { id state commit { hash } } stack { id } isModule } }"
        );
    }

    #[tokio::test]
    async fn root_args_render_before_the_filter() {
        let q = GraphqlSelect::new()
            .with_root_field("searchRuns")
            .with_root_args(json!({ "input": {} }))
            .with_field("run.id")
            .render()
            .await
            .unwrap();
        assert_eq!(q.query, "query { searchRuns(input: {}) { run { id } } }");
    }
}

#[cfg(test)]
mod envelope_tests {
    use super::*;
    use serde_json::json;

    /// The envelope has to shape the query too — asking for `run` at the
    /// root of a connection field is a server-side error, and the decoder
    /// would then be reading a path the query never selected.
    #[tokio::test]
    async fn response_path_wraps_the_selection_set() {
        let q = GraphqlSelect::new()
            .with_root_field("searchRuns")
            .with_root_args(json!({ "input": {} }))
            .with_response_path(vec!["edges".into(), "node".into()])
            .with_field("run.id")
            .with_field("stack.id")
            .render()
            .await
            .unwrap();
        assert_eq!(
            q.query,
            "query { searchRuns(input: {}) { edges { node { run { id } stack { id } } } } }"
        );
    }
}
