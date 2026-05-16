//! Table reference system for relationships between tables.
//!
//! The `Reference` trait describes a relationship (field names, target factory).
//! Same-persistence resolution happens in `Table::get_ref_from_row` via
//! `Reference::resolve_from_row` — the join value is read out of a known source
//! row and pushed as a plain eq-condition on the target.
//!
//! Two concrete types:
//! - `HasOne` — foreign key on source table (e.g. Client.bakery_id → Bakery)
//! - `HasMany` — foreign key on target table (e.g. Bakery → Client.bakery_id)
//!
//! Cross-persistence references live at the Vista layer (`Vista::with_foreign`),
//! not here. Typed `Table<T, E>` is single-backend by construction.

use std::any::Any;

use vantage_core::Result;

use crate::any::AnyTable;

pub mod many;
pub mod one;

pub use many::HasMany;
pub use one::HasOne;

/// Whether traversing a relation yields one record or a collection.
///
/// CLIs and UIs render the two differently — record card vs list grid —
/// and `Vista::list_references` surfaces this to callers without
/// requiring them to introspect the underlying `Reference` impl type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    One,
    Many,
}

/// Describes a relationship between two tables.
pub trait Reference: Send + Sync {
    /// Given source and target id field names, return (source_column, target_column).
    fn columns(&self, source_id: &str, target_id: &str) -> (String, String);

    /// Produce a fresh target table (no conditions applied).
    fn build_target(&self, data_source: &dyn Any) -> Box<dyn Any>;

    /// Cardinality of this relation — `One` for `HasOne`, `Many` for `HasMany`.
    fn cardinality(&self) -> Cardinality;

    /// Resolve traversal using a known source row. Returns the target table
    /// (entity type erased to `EmptyEntity`) wrapped in `Box<dyn Any>`, with
    /// one eq-condition applied that selects the related rows.
    ///
    /// `data_source` is `&T` for the source's `TableSource`; `source_id_field`
    /// is the name of the source table's id column (needed by `HasMany` to
    /// pull the join value out of the row); `source_row` is `&Record<T::Value>`.
    /// `HasOne` ignores `source_id_field` and reads its stored `foreign_key`
    /// instead.
    ///
    /// Callers immediately downcast the result to `Table<T, EmptyEntity>`.
    fn resolve_from_row(
        &self,
        data_source: &dyn Any,
        source_id_field: &str,
        source_row: &dyn Any,
    ) -> Result<Box<dyn Any>>;

    /// Resolve this reference and return an AnyTable.
    ///
    /// Legacy path used by `Table::get_ref` / `get_ref_as` / `get_subquery_as`.
    /// Slated for deletion in Stage 9 alongside `AnyTable`; new callers should
    /// use `resolve_from_row` (typed) or `Vista::get_ref` (erased) instead.
    fn resolve_as_any(&self, source_table: &dyn Any) -> Result<AnyTable>;

    /// Type name of the target table (for error messages).
    fn target_type_name(&self) -> &'static str;
}
