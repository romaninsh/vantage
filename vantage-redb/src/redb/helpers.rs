//! Small utilities shared across the read/write paths.

use indexmap::IndexMap;
use std::collections::HashSet;

use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::redb::Redb;
use crate::types::AnyRedbType;

/// Resolve the table's id column name, falling back to `"id"`.
pub(crate) fn id_column_name<T: TableSource, E: Entity<T::Value>>(table: &Table<T, E>) -> String {
    table
        .id_field()
        .map(|c| ColumnLike::name(c).to_string())
        .unwrap_or_else(|| "id".to_string())
}

/// Set of column names that carry the `Indexed` flag. The id column is
/// implicitly available without the flag because the main table is already
/// keyed by id.
pub(crate) fn indexed_columns<E>(table: &Table<Redb, E>) -> HashSet<String>
where
    E: Entity<AnyRedbType>,
{
    table
        .columns()
        .iter()
        .filter(|(_, c)| c.flags().contains(&ColumnFlag::Indexed))
        .map(|(name, _)| name.clone())
        .collect()
}

/// Apply pagination skip/limit to a fully loaded result set. We always
/// post-filter rather than push pagination into the iteration so the
/// "first condition seeds candidates" algorithm in `query.rs` doesn't
/// have to know about pagination.
pub(crate) fn paginate<E>(
    table: &Table<Redb, E>,
    out: IndexMap<String, Record<AnyRedbType>>,
) -> IndexMap<String, Record<AnyRedbType>>
where
    E: Entity<AnyRedbType>,
{
    if let Some(p) = table.pagination() {
        let skip = p.skip() as usize;
        let limit = p.limit() as usize;
        out.into_iter().skip(skip).take(limit).collect()
    } else {
        out
    }
}

/// Project a record's indexed (column, value) pairs as borrowed slices —
/// used by both the write path (insert/replace/patch/delete) and the
/// trait impl wrapper.
pub(crate) fn collect_indexed_pairs<'a>(
    record: &'a Record<AnyRedbType>,
    indexed: &HashSet<String>,
) -> Vec<(&'a str, &'a AnyRedbType)> {
    record
        .iter()
        .filter(|(name, _)| indexed.contains(name.as_str()))
        .map(|(n, v)| (n.as_str(), v))
        .collect()
}
