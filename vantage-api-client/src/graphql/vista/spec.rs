//! YAML-facing types for the GraphQL Vista driver.
//!
//! ```yaml
//! name: launches
//! id_column: id
//! columns:
//!   id:
//!     type: string
//!     flags: [id]
//!   mission_name:
//!     type: string
//!     flags: [title]
//!   launch_year:
//!     type: int
//! graphql:
//!   root_field: launches      # optional, defaults to `name`
//!   dialect: generic          # generic | hasura, optional
//!   filter_arg: find          # optional, dialect default otherwise
//! ```
//!
//! A user-maintained schema YAML drives `GraphqlApiVistaFactory::build_from_spec`
//! — the alternative to runtime introspection.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

/// Top-level table block — flattens into `VistaSpec` so YAML keys land
/// at the spec root under `graphql:`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphqlTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphql: Option<GraphqlBlock>,
}

/// Per-table GraphQL settings under the YAML `graphql:` key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphqlBlock {
    /// Override the YAML `name` when the Query field uses a different
    /// label (e.g. `name: orders` but the schema exposes `ordersList`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_field: Option<String>,

    /// `"generic"` or `"hasura"`. Unknown values fall back to the
    /// API-level default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dialect: Option<String>,

    /// Override the filter argument name (`where` / `find` / `filter`).
    /// Falls back to the dialect default if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_arg: Option<String>,
}

/// Per-column block — flattens into `ColumnSpec`. Optional today;
/// landing the shape now leaves room for future overrides without a
/// schema break.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphqlColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graphql: Option<GraphqlColumnBlock>,
}

/// Per-column GraphQL block under the YAML `graphql:` key on a column.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphqlColumnBlock {
    /// Server-side field name when it differs from the spec column key.
    /// The query renderer doesn't yet emit aliases — recorded for
    /// future use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

pub type NoGraphqlExtras = NoExtras;
pub type GraphqlApiVistaSpec = VistaSpec<GraphqlTableExtras, GraphqlColumnExtras, NoExtras>;
