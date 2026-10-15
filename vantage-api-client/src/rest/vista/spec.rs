//! YAML-facing types for the REST Vista driver. Only the table-level `api:`
//! block is driver-specific; columns and references use the universal
//! `VistaSpec` keys. Every key is listed in the book's "HTTP APIs" chapter.

use serde::{Deserialize, Serialize};
use vantage_vista::{NoExtras, VistaSpec};

/// Table-level driver extras: flattens into `VistaSpec`, so the `api:` key
/// sits at the spec root.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ApiTableExtras {
    /// The table's `api:` block, absent when every setting is defaulted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<ApiTableBlock>,
}

impl ApiTableExtras {
    /// Extras carrying `block` as the table's `api:` block.
    pub fn new(block: ApiTableBlock) -> Self {
        Self { api: Some(block) }
    }
}

/// Per-table REST settings under the YAML `api:` key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ApiTableBlock {
    /// URL path under the data source's base URL. May contain
    /// `{placeholder}` segments that `RestApi` substitutes from
    /// eq-conditions at request time; non-matching eq-conditions
    /// become query params. Defaults to `spec.name` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

impl ApiTableBlock {
    /// Set [`endpoint`](Self::endpoint).
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }
}

/// Column-level driver extras: none beyond the universal column spec.
pub type ApiColumnExtras = NoExtras;
/// Reference-level driver extras: none beyond the universal reference spec.
pub type ApiReferenceExtras = NoExtras;

/// A REST table described in YAML.
pub type RestApiVistaSpec = VistaSpec<ApiTableExtras, ApiColumnExtras, ApiReferenceExtras>;
