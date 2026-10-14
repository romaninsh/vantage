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
use vantage_vista::{Reference, ReferenceKind};

use crate::{table::Table, traits::table_source::TableSource};

/// A contained (embedded-in-row) relation declared on a [`Table`].
pub struct ContainedRelation<T: TableSource> {
    name: String,
    host_column: String,
    /// `ContainsOne` or `ContainsMany`: only [`one`](ContainedRelation::one)
    /// and [`many`](ContainedRelation::many) set it.
    kind: ReferenceKind,
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
    /// One record embedded as an object in `host_column`; its id is `"0"`.
    pub fn one(
        name: impl Into<String>,
        host_column: impl Into<String>,
        build_target: impl Fn(T) -> Table<T, EmptyEntity> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            host_column: host_column.into(),
            kind: ReferenceKind::ContainsOne,
            id_column: None,
            build_target: Arc::new(build_target),
        }
    }

    /// Records embedded as an array in `host_column`, each addressed by its
    /// `id_column` field (`None`: by position).
    pub fn many(
        name: impl Into<String>,
        host_column: impl Into<String>,
        id_column: Option<String>,
        build_target: impl Fn(T) -> Table<T, EmptyEntity> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            host_column: host_column.into(),
            kind: ReferenceKind::ContainsMany,
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

    pub fn kind(&self) -> ReferenceKind {
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

    /// The relation as a Vista reference of a contained kind, for metadata.
    /// The records' columns and id column are materialized at traversal from
    /// `build_target`.
    pub fn reference(&self) -> Reference {
        if self.kind == ReferenceKind::ContainsMany {
            Reference::contains_many(&self.name, &self.host_column)
        } else {
            Reference::contains_one(&self.name, &self.host_column)
        }
    }
}
