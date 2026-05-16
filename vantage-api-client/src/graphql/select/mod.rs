//! GraphQL SELECT-style query builder.
//!
//! Accumulates a root field, a selection set, filter conditions, sort,
//! limit/skip, and optional sub-selections for nested relationships.
//! `render()` produces a `(query_doc, variables)` pair that
//! `GraphqlApi::post_graphql` consumes directly.
//!
//! Two render modes are supported via `FilterDialect`:
//! * `Hasura` — `(where: { field: { _eq: v } }, limit: $limit, ...)`
//! * `Generic` — `(find: { field: v }, limit: $limit, ...)` — used by
//!   hand-rolled schemas like the public SpaceX API.

pub mod builder;
pub mod impls;
pub mod render;

use vantage_expressions::Order;

use crate::graphql::condition::{FilterDialect, GraphqlCondition};

/// GraphQL query builder — produces a query document + variables map.
///
/// Construct via `GraphqlSelect::new()` and the chainable builders, or
/// via `Selectable::with_*` methods. Designed to be generic in shape:
/// most servers wrap their list-field with a filter arg (`where:`,
/// `find:`, …) plus pagination args, and the renderer handles both
/// dialects we ship with.
#[derive(Clone, Debug)]
pub struct GraphqlSelect {
    /// Root field on the `Query` type — `launches`, `usersList`, etc.
    pub root_field: Option<String>,
    /// Optional operation name (e.g. `query GetLaunches { … }`).
    pub operation_name: Option<String>,
    /// Selected scalar fields. Empty = use the schema's id field as a
    /// fallback so we at least get *something* back.
    pub fields: Vec<String>,
    /// Nested selection sets for relationship traversal. Built up by
    /// the Phase 6 `with_many`/`with_one` paths.
    pub sub_selections: Vec<(String, GraphqlSelect)>,
    /// Conditions combined into the filter argument.
    pub conditions: Vec<GraphqlCondition>,
    /// Ordering specification. `Hasura` renders these as `order_by:`;
    /// other dialects ignore them (or error) until the schema map
    /// provides a per-table override.
    pub sort: Vec<(String, Order)>,
    /// `GROUP BY` fields. Most GraphQL servers don't expose grouping at
    /// the query level — we record these for round-trip and let the
    /// renderer decide whether to surface them.
    pub group_by: Vec<String>,
    /// `LIMIT`. Emitted as a `$limit: Int` variable.
    pub limit: Option<i64>,
    /// Offset. Emitted as a `$offset: Int` variable.
    pub skip: Option<i64>,
    /// Distinct flag — meaningful only when the schema supports it
    /// (Hasura's `distinct_on:`). Plumbed through but ignored by the
    /// default renderer until a per-dialect path is added.
    pub distinct: bool,
    /// Filter dialect — drives how `conditions` get rendered into a
    /// GraphQL argument. Defaults to `Generic` so the cheapest schemas
    /// work without configuration.
    pub dialect: FilterDialect,
    /// Override for the filter argument name. Defaults to `where`
    /// (Hasura) or `find` (Generic).
    pub filter_arg_name: Option<String>,
}

impl GraphqlSelect {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for GraphqlSelect {
    fn default() -> Self {
        Self {
            root_field: None,
            operation_name: None,
            fields: Vec::new(),
            sub_selections: Vec::new(),
            conditions: Vec::new(),
            sort: Vec::new(),
            group_by: Vec::new(),
            limit: None,
            skip: None,
            distinct: false,
            dialect: FilterDialect::Generic,
            filter_arg_name: None,
        }
    }
}
