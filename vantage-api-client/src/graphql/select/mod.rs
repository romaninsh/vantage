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

mod aliases;
mod builder;
mod count;
mod impls;
mod names;
mod render;

pub use render::RenderedQuery;

use indexmap::IndexMap;
use vantage_expressions::Order;

use crate::graphql::condition::{FilterDialect, GraphqlCondition};

/// GraphQL query builder — produces a query document + variables map.
///
/// Construct via [`GraphqlSelect::new`] and the chainable `with_*`
/// builders, or through the `Selectable` trait (`table.select()`). Designed
/// to be generic in shape: most servers wrap their list field with a
/// filter arg (`where:`, `find:`, …) plus pagination args, and the
/// renderer handles both dialects shipped here.
///
/// Grouping and `DISTINCT` have no GraphQL spelling: the `Selectable`
/// methods for them are no-ops.
#[derive(Clone, Debug, Default)]
pub struct GraphqlSelect {
    /// Root field on the `Query` type — `launches`, `usersList`, etc.
    pub(crate) root_field: Option<String>,
    /// Optional operation name (e.g. `query GetLaunches { … }`).
    pub(crate) operation_name: Option<String>,
    /// Selected fields. A dotted path (`run.commit.hash`) groups into a
    /// nested selection set at render time, so a schema that nests its
    /// scalars needs no relation machinery to read them.
    pub(crate) fields: Vec<String>,
    /// Column name → server field name, for columns whose wire name
    /// differs. The selection set asks for `name: field` (a GraphQL alias,
    /// so the response comes back under `name`), and conditions and sort
    /// keys are renamed to `field` on the way out.
    pub(crate) aliases: IndexMap<String, String>,
    /// Literal arguments always passed to the root field — the mandatory
    /// `input: {}` on Spacelift's `searchRuns(input: SearchInput!)`, say.
    /// Rendered inline alongside any filter/pagination arguments.
    pub(crate) root_args: Option<serde_json::Value>,
    /// Envelope the rows sit inside, e.g. `["edges", "node"]` for a Relay
    /// connection. The selection set is wrapped in these on the way out
    /// and the response is unwrapped through them on the way back, so the
    /// two can never drift apart.
    pub(crate) response_path: Vec<String>,
    /// Nested selection sets for relationship traversal.
    pub(crate) sub_selections: Vec<(String, GraphqlSelect)>,
    /// Conditions combined into the filter argument.
    pub(crate) conditions: Vec<GraphqlCondition>,
    /// Ordering. `Hasura` renders these as `order_by:`; `Generic` has no
    /// spelling for a sort and leaves them out.
    pub(crate) sort: Vec<(String, Order)>,
    /// `LIMIT`. Emitted as a `$limit: Int` variable.
    pub(crate) limit: Option<i64>,
    /// Offset. Emitted as a `$offset: Int` variable.
    pub(crate) skip: Option<i64>,
    /// Filter dialect — drives how `conditions` get rendered into a
    /// GraphQL argument. Defaults to `Generic` so the cheapest schemas
    /// work without configuration.
    pub(crate) dialect: FilterDialect,
    /// Override for the filter argument name. Defaults to `where`
    /// (Hasura) or `find` (Generic).
    pub(crate) filter_arg_name: Option<String>,
}

impl GraphqlSelect {
    /// An empty select: no root field, no fields, Generic dialect.
    pub fn new() -> Self {
        Self::default()
    }

    /// The root field on the `Query` type, once set.
    pub fn root_field(&self) -> Option<&str> {
        self.root_field.as_deref()
    }

    /// The selected field paths, in selection order.
    pub fn fields(&self) -> &[String] {
        &self.fields
    }

    /// The conditions combined into the filter argument.
    pub fn conditions(&self) -> &[GraphqlCondition] {
        &self.conditions
    }

    /// The filter dialect conditions render in.
    pub fn dialect(&self) -> FilterDialect {
        self.dialect
    }
}
