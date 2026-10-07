//! Write-path steps shared by the SQL dialects' `TableSource` impls: the
//! checks that confine a by-id write to the table's conditions, and the
//! errors they raise.

use std::fmt::Debug;

use vantage_core::{Result, VantageError, error};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::set_writes::{SetWrites, id_exists_anywhere, row_in_set};

/// Checks before an insert of `id`. `Some(row)` when the set already holds
/// `id`: the insert is a no-op that returns the stored row. On a conditioned
/// table, an id held by a row outside the set, or a record that would not
/// belong to the set, is a Conflict.
pub(crate) async fn guard_insert<T, E>(
    db: &T,
    table: &Table<T, E>,
    id: &T::Id,
    record: &Record<T::Value>,
) -> Result<Option<Record<T::Value>>>
where
    T: SetWrites,
    E: Entity<T::Value>,
{
    if let Some(existing) = db.get_table_value(table, id).await? {
        return Ok(Some(existing));
    }
    if table.conditions().next().is_some() {
        if id_exists_anywhere(db, table, id).await? {
            return Err(held_outside(table, id));
        }
        require_fits(db, table, id, record).await?;
    }
    Ok(None)
}

/// Settle a failed insert of `id`. Another writer may have taken the id
/// between [`guard_insert`] and the INSERT: a row now in the set is returned
/// as if inserted, a row outside it is a Conflict, anything else is `err`.
pub(crate) async fn settle_failed_insert<T, E>(
    db: &T,
    table: &Table<T, E>,
    id: &T::Id,
    err: VantageError,
) -> Result<Record<T::Value>>
where
    T: SetWrites,
    E: Entity<T::Value>,
{
    if let Some(existing) = db.get_table_value(table, id).await? {
        return Ok(existing);
    }
    if id_exists_anywhere(db, table, id).await? {
        return Err(held_outside(table, id));
    }
    Err(err)
}

/// Checks before a replace of `id` on a conditioned table: an id held only
/// outside the set, or a record that would not belong to it, is a Conflict.
pub(crate) async fn guard_replace<T, E>(
    db: &T,
    table: &Table<T, E>,
    id: &T::Id,
    record: &Record<T::Value>,
) -> Result<()>
where
    T: SetWrites,
    E: Entity<T::Value>,
{
    if table.conditions().next().is_none() {
        return Ok(());
    }
    let in_set = db.get_table_value(table, id).await?.is_some();
    if !in_set && id_exists_anywhere(db, table, id).await? {
        return Err(held_outside(table, id));
    }
    require_fits(db, table, id, record).await
}

/// Checks before a patch of `id`: the row must be in the set (NotFound
/// otherwise), and on a conditioned table the patched row must stay in it
/// (Conflict otherwise).
pub(crate) async fn guard_patch<T, E>(
    db: &T,
    table: &Table<T, E>,
    id: &T::Id,
    partial: &Record<T::Value>,
) -> Result<()>
where
    T: SetWrites,
    E: Entity<T::Value>,
{
    let Some(mut merged) = db.get_table_value(table, id).await? else {
        return Err(patch_not_found(table, id));
    };
    if table.conditions().next().is_some() {
        for (k, v) in partial.iter() {
            merged.insert(k.clone(), v.clone());
        }
        if !row_in_set(db, table, &merged).await? {
            return Err(patch_leaves_set(table, id));
        }
    }
    Ok(())
}

/// Check before an insert whose id the database generates: the record must
/// belong to the set (Conflict otherwise).
pub(crate) async fn guard_new_row<T, E>(
    db: &T,
    table: &Table<T, E>,
    record: &Record<T::Value>,
) -> Result<()>
where
    T: SetWrites,
    E: Entity<T::Value>,
{
    if !row_in_set(db, table, record).await? {
        return Err(new_row_not_in_set(table));
    }
    Ok(())
}

/// The record stored under `id` must satisfy the table's conditions.
async fn require_fits<T, E>(
    db: &T,
    table: &Table<T, E>,
    id: &T::Id,
    record: &Record<T::Value>,
) -> Result<()>
where
    T: SetWrites,
    E: Entity<T::Value>,
{
    let mut row = record.clone();
    row.insert(table.id_field_name(), T::id_cell(table, id));
    if !row_in_set(db, table, &row).await? {
        return Err(not_in_set(table, id));
    }
    Ok(())
}

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
