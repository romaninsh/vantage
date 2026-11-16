//! `table(name)`'s resolver over the engine's store, and `table()`'s default
//! to the calling sim's def table.

use std::collections::HashMap;
use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_memory::{MemoryStore, MemoryVistaFactory, MemoryVistaSpec};
use vantage_rhai::rhai::Engine;
use vantage_vista::rhai::Handle;
use vantage_vista::{ColumnFlag, ColumnSpec, Vista, VistaFactory, VistaResolver};

use crate::FakerColumn;
use crate::sim::current::with;

/// Resolve a table name to a fresh Vista over `store`. One resolver per
/// engine, built in `SimEngineBuilder::start`. `columns` is the same
/// per-table declared column list `fake_row()` reads, so a declared column
/// is orderable and searchable too.
///
/// A name the store has no table for is not-found; the resolver never
/// creates a table.
pub(in crate::sim) fn memory_resolver(
    store: MemoryStore,
    columns: &HashMap<String, Vec<FakerColumn>>,
) -> Arc<dyn VistaResolver> {
    let factory = MemoryVistaFactory::new(store.clone());
    let columns = columns.clone();
    Arc::new(move |name: &str| -> Result<Vista> {
        if !store.table_names().iter().any(|n| n == name) {
            return Err(error!("no table in this store", table = name).mark_not_found());
        }
        let id_column = store.table(name).id_column().to_string();
        let declared = columns.get(name).map(Vec::as_slice).unwrap_or_default();
        factory.build_from_spec(table_spec(name, &id_column, declared))
    })
}

/// Every declared column, searchable (and, as every memory column,
/// orderable), so a sim can `sort`/`search` by what it declared for
/// `fake_row()`. A `where` on an undeclared column is an error once the
/// table declares any column; a table that declares none gets only its id
/// column, takes a `where` on any column, but no `sort`.
///
/// A column's type here is a generation hint (a `faker:` generator decides
/// the cell's real form), and generated rows are stored as given, so every
/// column is untyped: sim writes are stored as given too.
fn table_spec(name: &str, id_column: &str, declared: &[FakerColumn]) -> MemoryVistaSpec {
    let mut spec = MemoryVistaSpec::new(name);
    spec.id_column = Some(id_column.to_string());
    for col in declared {
        let mut column = ColumnSpec::new();
        column.flags.push(ColumnFlag::Searchable);
        if col.name == id_column {
            column.flags.push(ColumnFlag::Id);
        }
        spec.columns.insert(col.name.clone(), column);
    }
    spec
}

/// `table()`: the calling sim's own def table, narrowed and resolved like
/// any `table(name)` handle through the engine's own `DataVocab` resolver.
pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("table", || {
        with(|c| Ok(Handle::named(c.kind().def.table.clone())))
    });
}
