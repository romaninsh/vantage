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
//! Cross-persistence references go through a `vantage_vista::Catalog`,
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

    /// This relation as Vista metadata: a join named `name` onto `target`.
    /// `source_id` is the source column a `has_many` reads its join value
    /// from; a `has_one` ignores it and constrains the target's id column,
    /// resolved at traversal. Only joins implement this trait, so the result
    /// is never a contained kind.
    fn vista_reference(
        &self,
        name: &str,
        target: String,
        source_id: &str,
    ) -> vantage_vista::Reference;

    /// Produce a fresh target table (no conditions applied), wrapped in
    /// `Box<dyn Any>` so callers can downcast back to the concrete
    /// `Table<T, TargetE>`. Used by [`crate::table::Table::get_ref_as`] and
    /// [`crate::table::Table::get_subquery_as`] to build the target before
    /// applying the join condition.
    fn build_target(&self, data_source: &dyn Any) -> Box<dyn Any>;

    /// Produce a fresh target table with its entity type erased to
    /// `EmptyEntity`, wrapped in `Box<dyn Any>` for downcast to
    /// `Table<T, EmptyEntity>`. Unlike [`build_target`](Self::build_target),
    /// the caller need not know the concrete target entity type — used by
    /// implicit-reference traversal, which walks relations by name.
    fn build_target_erased(&self, data_source: &dyn Any) -> Box<dyn Any>;

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

    /// The table name of the target built over `data_source` (`&T`). It
    /// names the relation's target in Vista metadata, which a catalog
    /// resolves by name.
    fn target_table_name(&self, data_source: &dyn Any) -> String;
}
