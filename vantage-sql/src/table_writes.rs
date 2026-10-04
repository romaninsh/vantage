//! Write-path steps shared by the SQL dialects' `TableSource` impls.

use std::fmt::Debug;

use vantage_core::{Result, error};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

/// Re-read a row after its UPDATE. The UPDATE itself doesn't report a
/// missing row, so a row that isn't there afterwards is NotFound.
pub(crate) async fn refetch_after_patch<T, E>(
    source: &T,
    table: &Table<T, E>,
    id: &T::Id,
) -> Result<Record<T::Value>>
where
    T: TableSource,
    T::Id: Debug,
    E: Entity<T::Value>,
{
    source
        .get_table_value(table, id)
        .await?
        .ok_or_else(|| error!("Row not found after patch", id = id.clone()).mark_not_found())
}
