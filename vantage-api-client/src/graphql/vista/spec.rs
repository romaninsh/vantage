//! YAML-facing types for the GraphQL Vista driver: a table-level `graphql:`
//! block and a column-level one. A hand-kept spec drives
//! `GraphqlApiVistaFactory::build_from_spec` in place of introspection. Every
//! key is listed in the book's "HTTP APIs" chapter.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

use crate::graphql::api::CountMode;
use crate::graphql::condition::FilterDialect;

/// Top-level table block — flattens into `VistaSpec` so YAML keys land
/// at the spec root under `graphql:`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct GraphqlTableExtras {
    /// The table's `graphql:` block, absent when every setting is
    /// defaulted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphql: Option<GraphqlBlock>,
}

impl GraphqlTableExtras {
    /// Extras carrying `block` as the table's `graphql:` block.
    pub fn new(block: GraphqlBlock) -> Self {
        Self {
            graphql: Some(block),
        }
    }
}

/// Per-table GraphQL settings under the YAML `graphql:` key.
///
/// Every field is optional; build one from [`GraphqlBlock::default`] with
/// the `with_*` setters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct GraphqlBlock {
    /// Override the YAML `name` when the Query field uses a different
    /// label (e.g. `name: orders` but the schema exposes `ordersList`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_field: Option<String>,

    /// Hasura only: the aggregate field queried by `count: aggregate`.
    /// Defaults to `<root_field>_aggregate`, Hasura's own naming.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count_field: Option<String>,

    /// `generic` or `hasura`; any other value fails to parse. Absent keeps
    /// the dialect the `GraphqlApi` was built with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialect: Option<FilterDialect>,

    /// Override the filter argument name (`where` / `find` / `filter`).
    /// Falls back to the dialect default if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_arg: Option<String>,

    /// Literal arguments always passed to the root field, e.g.
    /// `args: { input: {} }` for Spacelift's `searchRuns(input:
    /// SearchInput!)` — a non-null argument the query is invalid without.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,

    /// Dotted path walked into the root field's value to reach the rows.
    /// `edges.node` unwraps a Relay-style connection; omit it when the
    /// root field returns the array directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_path: Option<String>,

    /// Whether conditions may be pushed into the filter argument. Default:
    /// on. A root field taking no arguments at all needs `filter: false`,
    /// or every narrowing renders a rejected query; conditions are then
    /// applied to the fetched rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<bool>,

    /// Whether `order_by:` may be rendered. Default: on for Hasura only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<bool>,

    /// Whether a quicksearch may render as an OR of `_ilike`s. Default: on
    /// for Hasura only, and never with `filter: false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<bool>,

    /// `paginate: true` sends `limit:`/`offset:` and enables offset paging
    /// (`fetch_window`). Off by default, and
    /// not offered with `filter: false`, where local filtering would cut
    /// server pages short.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paginate: Option<bool>,

    /// `list` (default) counts by listing rows; `aggregate` (Hasura only)
    /// asks `count_field` for `aggregate { count }`, which needs a role
    /// allowed to aggregate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<CountMode>,
}

impl GraphqlBlock {
    /// Set [`root_field`](Self::root_field).
    pub fn with_root_field(mut self, root_field: impl Into<String>) -> Self {
        self.root_field = Some(root_field.into());
        self
    }

    /// Set [`count_field`](Self::count_field).
    pub fn with_count_field(mut self, count_field: impl Into<String>) -> Self {
        self.count_field = Some(count_field.into());
        self
    }

    /// Set [`dialect`](Self::dialect).
    pub fn with_dialect(mut self, dialect: FilterDialect) -> Self {
        self.dialect = Some(dialect);
        self
    }

    /// Set [`filter_arg`](Self::filter_arg).
    pub fn with_filter_arg(mut self, filter_arg: impl Into<String>) -> Self {
        self.filter_arg = Some(filter_arg.into());
        self
    }

    /// Set [`args`](Self::args).
    pub fn with_args(mut self, args: serde_json::Value) -> Self {
        self.args = Some(args);
        self
    }

    /// Set [`response_path`](Self::response_path).
    pub fn with_response_path(mut self, response_path: impl Into<String>) -> Self {
        self.response_path = Some(response_path.into());
        self
    }

    /// Set [`filter`](Self::filter).
    pub fn with_filter(mut self, on: bool) -> Self {
        self.filter = Some(on);
        self
    }

    /// Set [`order`](Self::order).
    pub fn with_order(mut self, on: bool) -> Self {
        self.order = Some(on);
        self
    }

    /// Set [`search`](Self::search).
    pub fn with_search(mut self, on: bool) -> Self {
        self.search = Some(on);
        self
    }

    /// Set [`paginate`](Self::paginate).
    pub fn with_paginate(mut self, on: bool) -> Self {
        self.paginate = Some(on);
        self
    }

    /// Set [`count`](Self::count).
    pub fn with_count(mut self, count: CountMode) -> Self {
        self.count = Some(count);
        self
    }
}

/// Per-column driver extras — flattens into `ColumnSpec`, so the block
/// sits under the column's `graphql:` key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct GraphqlColumnExtras {
    /// The column's `graphql:` block, absent when every setting is
    /// defaulted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphql: Option<GraphqlColumnBlock>,
}

impl GraphqlColumnExtras {
    /// Extras carrying `block` as the column's `graphql:` block.
    pub fn new(block: GraphqlColumnBlock) -> Self {
        Self {
            graphql: Some(block),
        }
    }
}

/// Per-column GraphQL settings under the YAML `graphql:` key on a column.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct GraphqlColumnBlock {
    /// Server field the column reads, when it differs from the column
    /// name. The query selects it under a GraphQL alias (`name: field`),
    /// so rows still carry the column name; conditions, ordering and
    /// search on the column are sent as `field`. Both the column name and
    /// `field` must be plain GraphQL names (no dotted paths).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl GraphqlColumnBlock {
    /// Set [`field`](Self::field).
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }
}

/// A GraphQL table described in YAML.
pub type GraphqlApiVistaSpec = VistaSpec<GraphqlTableExtras, GraphqlColumnExtras, NoExtras>;
