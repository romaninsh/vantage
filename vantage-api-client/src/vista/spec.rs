//! YAML-facing types for the REST-API Vista driver.
//!
//! Only the table-level `api` block carries driver-specific fields.
//! Column and reference extras stay at `NoExtras` — the universal
//! `VistaSpec` already covers what's needed (column type/flags,
//! reference target/kind/foreign_key). URL templating lives on each
//! table's own `api.endpoint` and is shared across all traversals;
//! the URL builder peels matching eq-conditions into `{placeholder}`
//! segments and lets the rest fall through to the query string.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiTableExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<ApiTableBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiTableBlock {
    /// URL path under the data source's base URL. May contain
    /// `{placeholder}` segments that `RestApi` substitutes from
    /// eq-conditions at request time; non-matching eq-conditions
    /// become query params. Defaults to `spec.name` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

pub type ApiColumnExtras = NoExtras;
pub type ApiReferenceExtras = NoExtras;

/// Legacy alias for callers that already imported `NoApiExtras` from
/// the placeholder version of this module.
pub type NoApiExtras = NoExtras;

pub type RestApiVistaSpec = VistaSpec<ApiTableExtras, ApiColumnExtras, ApiReferenceExtras>;
