//! Render a `GraphqlSelect` into `(query_doc, variables)` for HTTP POST.
//!
//! The filter argument is rendered **inline** (as GraphQL value syntax)
//! to avoid having to know the server's input-object type names. Pagination
//! goes through typed variables (`$limit: Int`, `$offset: Int`) which the
//! GraphQL spec covers without any schema lookup.
//!
//! That trade-off keeps the renderer schema-agnostic: no input-type names
//! are needed, at the cost of the filter not being a reusable variable.

use serde_json::{Map, Value};
use vantage_core::{Result, error};
use vantage_expressions::Order;

use crate::graphql::condition::{FilterDialect, GraphqlCondition};
use crate::graphql::name::validate;
use crate::graphql::select::GraphqlSelect;

/// The rendered output of [`GraphqlSelect::render`] — what
/// `GraphqlApi::post_graphql` consumes.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RenderedQuery {
    /// The query document.
    pub query: String,
    /// Values for the document's variables (`limit`, `offset`).
    pub variables: Map<String, Value>,
}

/// `order_by: [{a: asc, b: desc}]` for the given sort keys.
fn render_order_by(sort: &[(&str, Order)]) -> String {
    let entries: Vec<String> = sort
        .iter()
        .map(|(field, order)| format!("{}: {}", field, render_order(*order)))
        .collect();
    format!("order_by: [{{{}}}]", entries.join(", "))
}

/// AND the conditions into one, `None` when there are none.
fn combined(conditions: Vec<GraphqlCondition>) -> Option<GraphqlCondition> {
    match conditions.len() {
        0 => None,
        1 => conditions.into_iter().next(),
        _ => Some(GraphqlCondition::And(conditions)),
    }
}

impl GraphqlSelect {
    /// Produce a `(query, variables)` pair. Async because conditions
    /// may carry `Deferred` branches that resolve at fetch time.
    ///
    /// Every name spliced into the document is checked against the GraphQL
    /// `Name` grammar first; an invalid one is an error, never a request.
    pub async fn render(&self) -> Result<RenderedQuery> {
        let root = self
            .root_field
            .as_deref()
            .ok_or_else(|| error!("GraphqlSelect: root_field is required"))?;
        validate("root field", root)?;
        self.validate_selection_names()?;

        let mut variables = Map::new();
        let mut var_decls: Vec<String> = Vec::new();
        let mut args = self.render_root_and_filter_args().await?;

        if !self.sort.is_empty() && matches!(self.dialect, FilterDialect::Hasura) {
            args.push(render_order_by(&self.wire_sort()));
        }

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

        // Wrapped in the response envelope, so the query asks for rows
        // exactly where the decoder will look for them.
        let mut selection_set = render_selection_set(self).await?;
        for segment in self.response_path.iter().rev() {
            selection_set = format!("{{ {} {} }}", segment, selection_set);
        }

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

    /// The literal root arguments followed by the inline filter argument —
    /// what narrows the row set, as opposed to ordering or paging it. Shared
    /// by [`render`](Self::render) and the aggregate count query, so a count
    /// sees exactly the rows a list would.
    pub(crate) async fn render_root_and_filter_args(&self) -> Result<Vec<String>> {
        // Literal root arguments first, so a mandatory `input:` reads before
        // the filter it often has nothing to do with.
        let mut args = self.render_root_args()?;

        if let Some(condition) = combined(self.wire_conditions()) {
            let rendered = condition.render(self.dialect).await?;
            args.push(self.render_filter_arg(&rendered)?);
        }
        Ok(args)
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

        if let Some(condition) = combined(self.wire_conditions()) {
            let arg_name = self.filter_arg();
            match condition.render_preview(self.dialect) {
                Ok(value) => args.push(format!("{}: {}", arg_name, json_to_graphql_value(&value))),
                // A dialect that cannot spell this filter fails at send time
                // too. Surfacing that here is the point of previewing.
                Err(e) => args.push(format!("{}: <unrenderable: {}>", arg_name, e)),
            }
        }

        if !self.sort.is_empty() && matches!(self.dialect, FilterDialect::Hasura) {
            args.push(render_order_by(&self.wire_sort()));
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
        let mut parts = FieldTree::from_paths(&select.selection_paths()).into_parts();
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
    let mut parts = FieldTree::from_paths(&select.selection_paths()).into_parts();
    for (field, child) in &select.sub_selections {
        parts.push(format!("{}{}", field, preview_inline_subselection(child)));
    }
    format!("{{ {} }}", parts.join(" "))
}

/// Synchronous [`render_inline_subselection`], for [`GraphqlSelect::preview`].
fn preview_inline_subselection(child: &GraphqlSelect) -> String {
    let mut args: Vec<String> = Vec::new();
    if let Some(condition) = combined(child.wire_conditions()) {
        let arg_name = child.filter_arg();
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
        if let Some(condition) = combined(child.wire_conditions()) {
            let rendered = condition.render(child.dialect).await?;
            args.push(child.render_filter_arg(&rendered)?);
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

/// The body of a GraphQL string literal. Quote and backslash are escaped,
/// newline, carriage return and tab take their short forms, and every other
/// control character (U+0000–U+001F, and DEL) becomes `\u00XX` — a raw
/// control character is not valid inside a GraphQL string.
fn escape_string(s: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_ascii_control() => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn render_order(order: Order) -> &'static str {
    if order.ascending { "asc" } else { "desc" }
}
