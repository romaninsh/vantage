//! Conversions between the typed layer and the store: records, and a
//! table's conditions, orders and pagination as a [`Query`].

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_table::sorting::SortDirection as TableSort;
use vantage_table::table::Table;
use vantage_types::{Entity, Record};
use vantage_vista::SortDirection;

use super::MemoryDB;
use crate::eval::{MemoryCondition, Query};
use crate::store::Row;
use crate::types::AnyMemoryType;

pub(crate) fn to_cbor_record(record: &Record<AnyMemoryType>) -> Record<CborValue> {
    record
        .iter()
        .map(|(k, v)| (k.clone(), v.value().clone()))
        .collect()
}

pub(crate) fn from_cbor_record(record: &Record<CborValue>) -> Record<AnyMemoryType> {
    record
        .iter()
        .map(|(k, v)| (k.clone(), AnyMemoryType::untyped(v.clone())))
        .collect()
}

/// One column's cells; the row ids when `column` is `id_column`.
pub(crate) fn column_values(
    rows: &[(String, Row)],
    column: &str,
    id_column: &str,
) -> Vec<CborValue> {
    rows.iter()
        .filter_map(|(id, row)| match column == id_column {
            true => Some(CborValue::Text(id.clone())),
            false => row.get(column).cloned(),
        })
        .collect()
}

/// The table's conditions (deferred ones resolved), orders and pagination.
pub async fn table_query<E>(table: &Table<MemoryDB, E>) -> Result<Query>
where
    E: Entity<AnyMemoryType>,
{
    let mut q = Query::new();
    for c in table.conditions() {
        q = q.filter(c.clone());
    }
    for (key, dir) in table.orders() {
        let MemoryCondition::Column(path) = key else {
            return Err(error!("Order key must be a column"));
        };
        let dir = match dir {
            TableSort::Ascending => SortDirection::Ascending,
            TableSort::Descending => SortDirection::Descending,
        };
        q = q.order_by(path.clone(), dir);
    }
    if let Some(p) = table.pagination() {
        q = q.window(p.skip().max(0) as usize, Some(p.limit().max(0) as usize));
    }
    q.resolve().await
}
