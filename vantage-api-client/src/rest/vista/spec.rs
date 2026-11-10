//! YAML-facing types for the REST Vista driver. Only the table-level `api:`
//! block is driver-specific; columns and references use the universal
//! `VistaSpec` keys. Every key is listed in the book's "HTTP APIs" chapter.

use indexmap::IndexMap;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ApiTableBlock {
    /// URL path under the data source's base URL. May contain
    /// `{placeholder}` segments that `RestApi` substitutes from
    /// eq-conditions at request time; non-matching eq-conditions
    /// become query params. Defaults to `spec.name` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// How the table writes. Absent means the table has no insert.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert: Option<ApiInsertBlock>,
    /// `false` makes an insert-only table: no list, get or count, and no
    /// request is sent for one. Defaults to `true`.
    #[serde(default = "default_read", skip_serializing_if = "is_true")]
    pub read: bool,
}

impl Default for ApiTableBlock {
    fn default() -> Self {
        Self {
            endpoint: None,
            insert: None,
            read: true,
        }
    }
}

impl ApiTableBlock {
    /// Set [`endpoint`](Self::endpoint).
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }
}

/// How a REST table inserts, under `api.insert:`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ApiInsertBlock {
    /// HTTP method; only `POST` is accepted.
    #[serde(default = "default_method")]
    pub method: String,
    /// Path under the base URL; defaults to the table's endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Query params sent with every insert request.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub query: IndexMap<String, String>,
    /// Statuses that count as success. Empty means any 2xx.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accept: Vec<u16>,
    /// Records per request.
    #[serde(default = "default_batch")]
    pub batch: usize,
    /// Field whose value splits records into separate requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
    /// Request body template; absent sends the record as-is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Template for the idempotency key header value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Whether a retried request is safe to repeat.
    #[serde(default)]
    pub idempotent: bool,
    /// Compression applied to the request body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<Encoding>,
}

impl Default for ApiInsertBlock {
    fn default() -> Self {
        Self {
            method: default_method(),
            path: None,
            query: IndexMap::new(),
            accept: Vec::new(),
            batch: default_batch(),
            group_by: None,
            body: None,
            idempotency_key: None,
            idempotent: false,
            encoding: None,
        }
    }
}

/// Body compression for an insert request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoding {
    /// `Content-Encoding: gzip`.
    Gzip,
}

fn default_read() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

fn default_method() -> String {
    "POST".to_string()
}

fn default_batch() -> usize {
    1
}

/// Column-level driver extras: none beyond the universal column spec.
pub type ApiColumnExtras = NoExtras;
/// Reference-level driver extras: none beyond the universal reference spec.
pub type ApiReferenceExtras = NoExtras;

/// A REST table described in YAML.
pub type RestApiVistaSpec = VistaSpec<ApiTableExtras, ApiColumnExtras, ApiReferenceExtras>;
