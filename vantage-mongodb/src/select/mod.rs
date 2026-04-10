//! MongoDB SELECT (find) query builder.
//!
//! Accumulates collection name, field projection, filter conditions, sort,
//! limit/skip — all as native MongoDB types. Implements
//! `Selectable<AnyMongoType, MongoCondition>` so `table.select()` works.

pub mod builder;
pub mod impls;
pub mod pipeline;
pub mod render;

use crate::condition::MongoCondition;

/// MongoDB query builder — the equivalent of `SqliteSelect` / `SurrealSelect`.
///
/// Instead of rendering SQL, it accumulates native `bson::Document` parts
/// that map directly onto the `mongodb` driver's find/aggregate API.
#[derive(Debug, Clone, Default)]
pub struct MongoSelect {
    /// Collection name (equivalent to FROM).
    pub collection: Option<String>,
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
