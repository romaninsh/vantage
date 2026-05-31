//! Contained relation registration — a closure that builds the embedded
//! record's table, mirroring how [`HasOne`](super::HasOne) /
//! [`HasMany`](super::HasMany) carry a `build_target` for joined tables.
//!
//! The contained record's fields are typed in the parent's own type system (a
//! line's `product` is the driver's `Thing`, its `quantity` an `i64`), so the
//! closure has the same shape as `with_many`'s — `Fn(T) -> Table<T, E2>`. It's
//! evaluated lazily at traversal time; the driver runs its normal
//! column-harvesting routine on the result to derive the sub-Vista's schema.
//! The returned table carries a real data source it is never queried through —
//! only its column/relation schema is read; row data comes from the parent's
//! embedded column.

use std::sync::Arc;

use vantage_types::EmptyEntity;
use vantage_vista::{ContainedKind, ContainedSpec};

use crate::{table::Table, traits::table_source::TableSource};

/// A contained (embedded-in-row) relation declared on a [`Table`].
pub struct ContainedRelation<T: TableSource> {
    name: String,
    host_column: String,
    kind: ContainedKind,
    id_column: Option<String>,
    build_target: Arc<dyn Fn(T) -> Table<T, EmptyEntity> + Send + Sync>,
}

impl<T: TableSource> Clone for ContainedRelation<T> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            host_column: self.host_column.clone(),
            kind: self.kind,
            id_column: self.id_column.clone(),
            build_target: self.build_target.clone(),
        }
    }
}

impl<T: TableSource + 'static> ContainedRelation<T> {
    pub fn new(
        name: impl Into<String>,
        host_column: impl Into<String>,
        kind: ContainedKind,
        id_column: Option<String>,
        build_target: impl Fn(T) -> Table<T, EmptyEntity> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            host_column: host_column.into(),
            kind,
            id_column,
            build_target: Arc::new(build_target),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn host_column(&self) -> &str {
        &self.host_column
    }

    pub fn kind(&self) -> ContainedKind {
        self.kind
    }

    pub fn id_column(&self) -> Option<&str> {
        self.id_column.as_deref()
    }

    /// Build the contained record's table (its schema), used at traversal to
    /// derive the sub-Vista's columns.
    pub fn build_target(&self, db: T) -> Table<T, EmptyEntity> {
        (self.build_target)(db)
    }

    /// Shape-only vista spec (name, host, kind, id — no columns) for metadata
    /// surfacing. Columns are materialized at traversal from `build_target`.
    pub fn spec(&self) -> ContainedSpec {
        let mut spec = ContainedSpec::new(&self.name, &self.host_column, self.kind);
        if let Some(id) = &self.id_column {
            spec = spec.with_id_column(id);
        }
        spec
    }
}
