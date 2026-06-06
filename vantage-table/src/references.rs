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
//! Cross-persistence references live in `vantage-vista-factory`'s `VistaCatalog`,
//! not here. Typed `Table<T, E>` is single-backend by construction.

use std::any::Any;

use vantage_core::Result;

pub mod contained;
pub mod many;
pub mod one;

pub use contained::ContainedRelation;
pub use many::HasMany;
pub use one::HasOne;

/// Describes a relationship between two tables.
pub trait Reference: Send + Sync {
    /// Given source and target id field names, return (source_column, target_column).
    fn columns(&self, source_id: &str, target_id: &str) -> (String, String);

    /// The foreign-key column carried by this relation. For `HasOne` it names a
    /// column on the *source* table (set to the related row's id after the
    /// related row is inserted); for `HasMany` it names a column on the *target*
    /// table (set to the parent's id when inserting each child). Drives nested
    /// insert at the Vista layer.
    fn foreign_key(&self) -> &str;

    /// Produce a fresh target table (no conditions applied), wrapped in
    /// `Box<dyn Any>` so callers can downcast back to the concrete
    /// `Table<T, TargetE>`. Used by [`crate::table::Table::get_ref_as`] and
    /// [`crate::table::Table::get_subquery_as`] to build the target before
    /// applying the join condition.
    fn build_target(&self, data_source: &dyn Any) -> Box<dyn Any>;

    /// Cardinality of this relation. `HasOne` if traversing yields at most
    /// one record (the FK lives on the source); `HasMany` if it can yield
    /// any number (the FK lives on the target). Surfaced by
    /// `Vista::list_references` so CLIs / UIs can pick a record-card vs
    /// list-grid renderer.
    fn cardinality(&self) -> vantage_vista::ReferenceKind;

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

    /// Type name of the target table (for error messages).
    fn target_type_name(&self) -> &'static str;
}
