//! Set membership — decides whether a row belongs to a table's narrowed set.
//!
//! Rows are checked in memory against the table's conditions. Deferred
//! conditions are resolved first, so a condition that cannot be evaluated
//! (e.g. search) fails the check instead of silently matching.

use vantage_core::Result;
use vantage_table::table::Table;
use vantage_types::{Entity, InvariantValue, Record};

use crate::condition::RedbCondition;
use crate::types::AnyRedbType;

/// Resolve every deferred condition of `table`. Call before opening a write
/// transaction; the result is checked synchronously with [`fits`].
pub(crate) async fn resolved_conditions<E>(
    table: &Table<crate::Redb, E>,
) -> Result<Vec<RedbCondition>>
where
    E: Entity<AnyRedbType>,
{
    futures_util::future::try_join_all(table.conditions().map(|c| c.clone().resolve())).await
}

/// True when `row` satisfies every resolved condition.
pub(crate) fn fits(conditions: &[RedbCondition], row: &Record<AnyRedbType>) -> bool {
    conditions.iter().all(|c| match c {
        RedbCondition::Eq { column, value } => {
            row.get(column.as_str()).is_some_and(|v| v.value_eq(value))
        }
        RedbCondition::In { column, values } => row
            .get(column.as_str())
            .is_some_and(|v| values.iter().any(|x| v.value_eq(x))),
        RedbCondition::Deferred(_) => false,
    })
}

/// The row with its id stored under the table's id column, so conditions on
/// the id column can be evaluated against it.
pub(crate) fn with_id(row: &Record<AnyRedbType>, id_column: &str, id: &str) -> Record<AnyRedbType> {
    let mut full = row.clone();
    full.insert(id_column.to_string(), AnyRedbType::new(id.to_string()));
    full
}
