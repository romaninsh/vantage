//! Write-path steps shared by the SQL dialects' `TableSource` impls.

use std::fmt::Debug;

use vantage_core::{Result, VantageError, error};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

/// Insert or replace of an id that a row outside the table's conditions holds.
pub(crate) fn held_outside<T, E>(table: &Table<T, E>, id: &T::Id) -> VantageError
where
    T: TableSource,
    T::Id: Debug,
    E: Entity<T::Value>,
{
    error!(
        "id is held by a row outside this set",
        table = table.table_name(),
        id = id
    )
    .mark_conflict()
}

/// A written row that would not satisfy the table's conditions.
pub(crate) fn not_in_set<T, E>(table: &Table<T, E>, id: &T::Id) -> VantageError
where
    T: TableSource,
    T::Id: Debug,
    E: Entity<T::Value>,
{
    error!(
        "record does not belong to this set",
        table = table.table_name(),
        id = id
    )
    .mark_conflict()
}

/// A patch whose merged row would no longer satisfy the table's conditions.
pub(crate) fn patch_leaves_set<T, E>(table: &Table<T, E>, id: &T::Id) -> VantageError
where
    T: TableSource,
    T::Id: Debug,
    E: Entity<T::Value>,
{
    error!(
        "patch would move the row out of this set",
        table = table.table_name(),
        id = id
    )
    .mark_conflict()
}

/// Patch of a row that is missing or outside the table's conditions.
pub(crate) fn patch_not_found<T, E>(table: &Table<T, E>, id: &T::Id) -> VantageError
where
    T: TableSource,
    T::Id: Debug,
    E: Entity<T::Value>,
{
    error!("Row not found", table = table.table_name(), id = id).mark_not_found()
}

/// A record for a generated id that would not satisfy the table's conditions.
pub(crate) fn new_row_not_in_set<T, E>(table: &Table<T, E>) -> VantageError
where
    T: TableSource,
    E: Entity<T::Value>,
{
    error!(
        "record does not belong to this set",
        table = table.table_name()
    )
    .mark_conflict()
}

/// Re-read a row after its UPDATE. The patch paths check the row is in the
/// set before updating, so this only fails when another writer deleted or
/// moved the row in between: the UPDATE doesn't report that, and a row that
/// isn't there afterwards is NotFound.
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
