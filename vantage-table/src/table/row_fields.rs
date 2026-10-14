//! The fields a table's returned rows carry, and the in-memory filter that
//! relies on them.

use indexmap::IndexMap;
use vantage_types::{Entity, Record};

use crate::{
    column::flags::ColumnFlag,
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// The declared columns a returned row carries, in declaration order:
    /// every column except those flagged
    /// [`RequestParam`](ColumnFlag::RequestParam), whose values go to the
    /// backend rather than come back from it. A `Hidden` column is still a
    /// row field.
    pub fn row_fields(&self) -> Vec<String> {
        self.columns()
            .iter()
            .filter(|(_, column)| !column.flags().contains(&ColumnFlag::RequestParam))
            .map(|(name, _)| name.clone())
            .collect()
    }
}

/// Keep the rows that match every `(field, wanted)` equality filter, for a
/// backend that filters fetched rows in memory.
///
/// A row holding `field` passes when `eq(value, wanted)`. A row without it
/// fails when `field` is one of `row_fields` (see [`Table::row_fields`]):
/// rows were promised to carry it. Any other field (a `RequestParam` column,
/// a URL placeholder, a parameter the table doesn't declare) went to the
/// backend, which applied it without returning it, so its absence passes.
pub fn retain_matching<V, W: ?Sized>(
    rows: &mut IndexMap<String, Record<V>>,
    filters: &[(&str, &W)],
    row_fields: &[String],
    eq: impl Fn(&V, &W) -> bool,
) {
    rows.retain(|_, row| {
        filters.iter().all(|(field, wanted)| match row.get(*field) {
            Some(value) => eq(value, wanted),
            None => !row_fields.iter().any(|f| f == field),
        })
    });
}
