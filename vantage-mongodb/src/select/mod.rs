//! MongoDB SELECT (find) query builder.
//!
//! Accumulates collection name, field projection, filter conditions, sort,
//! limit/skip — all as native MongoDB types. Implements
//! `Selectable<AnyMongoType, MongoCondition>` so `table.select()` works.

pub mod builder;
pub mod impls;
pub(crate) mod join;
pub mod pipeline;
pub mod render;

use crate::condition::MongoCondition;

pub(crate) use join::{JoinColumn, JoinHop, join_alias};

/// MongoDB query builder — the equivalent of `SqliteSelect` / `SurrealSelect`.
///
/// Instead of rendering SQL, it accumulates native `bson::Document` parts
/// that map directly onto the `mongodb` driver's find/aggregate API.
///
/// A select built from a typed table (`table.select()`) carries the table's
/// joined (dotted) columns as plain field names: running it through
/// `execute_select` does not lower them into `$lookup` stages. Reads through
/// the table (`list_values` and friends) and through a Vista do.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct MongoSelect {
    /// Collection name (equivalent to FROM).
    pub collection: Option<String>,
    /// Aggregation stages that run before the filter. Non-empty makes every
    /// read an `aggregate` over their output instead of a `find`.
    pub(crate) pre_stages: Vec<bson::Document>,
    /// Dotted columns read through `$lookup` stages; see the `join` module.
    /// Non-empty makes every read an `aggregate`.
    pub(crate) joins: Vec<JoinColumn>,
    /// Field names to include in projection. Empty = all fields.
    pub fields: Vec<String>,
    /// Filter conditions (combined with $and at resolve time).
    pub conditions: Vec<MongoCondition>,
    /// Sort specification: field name → 1 (asc) or -1 (desc).
    pub sort: Vec<(String, i32)>,
    /// GROUP BY fields (for aggregation pipeline).
    pub group_by: Vec<String>,
    /// Whether to return distinct results.
    pub distinct: bool,
    /// Max number of documents to return.
    pub limit: Option<i64>,
    /// Number of documents to skip.
    pub skip: Option<i64>,
}

impl MongoSelect {
    pub fn new() -> Self {
        Self::default()
    }
}
