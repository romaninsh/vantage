//! Checks on a whole [`DatasetGen`](super::DatasetGen) plan, run before any
//! table is seeded so a bad plan leaves the store untouched.

use std::collections::HashSet;

use vantage_core::{Result, error};
use vantage_memory::MemoryStore;

use super::order;
use super::table::TableGen;

/// Check every plan in `tables` against each other and against `store`, and
/// return the generation order.
///
/// Rejects a name declared twice, an invalid column generator, an invalid
/// fan-out or one on a column that is not a reference, a table already in
/// `store` under another id column, and — through the ordering — a
/// reference to an unknown table or a reference cycle.
pub(super) fn check(tables: &[TableGen], store: &MemoryStore) -> Result<Vec<usize>> {
    let existing: HashSet<String> = store.table_names().into_iter().collect();
    let mut names = HashSet::new();
    for plan in tables {
        let name = &plan.name;
        if !names.insert(name.as_str()) {
            return Err(error!("Table is declared twice", table = name));
        }
        for col in &plan.columns {
            if let Some(generator) = &col.generator {
                generator.validate().map_err(|reason| {
                    error!(
                        "Column generator is invalid",
                        table = name,
                        column = col.name,
                        reason = reason
                    )
                })?;
            }
        }
        if let Some(fan) = &plan.fan_out {
            fan.validate().map_err(|reason| {
                error!("Table fan_out is invalid", table = name, reason = reason)
            })?;
            if !plan.refs.iter().any(|r| r.column == fan.column) {
                return Err(error!(
                    "Table fan_out is not a reference column",
                    table = name,
                    column = fan.column
                ));
            }
        }
        if existing.contains(name) {
            let table = store.table(name);
            if table.id_column() != plan.id_column.as_str() {
                return Err(error!(
                    "Existing table has another id column",
                    table = name,
                    existing = table.id_column(),
                    planned = plan.id_column
                ));
            }
        }
    }
    order::generation_order(tables)
}
