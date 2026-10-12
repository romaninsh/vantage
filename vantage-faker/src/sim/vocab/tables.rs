//! `table(name)`'s resolver over the engine's store, and `table()`'s default
//! to the calling sim's def table.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use vantage_core::{Result, error};
use vantage_memory::{MemoryCatalog, MemoryStore};
use vantage_rhai::rhai::Engine;
use vantage_vista::rhai::Handle;
use vantage_vista::{Column, ColumnFlag, Vista, VistaMetadata, VistaResolver};

use crate::FakerColumn;
use crate::sim::current::with;

/// Resolve a table name to a fresh Vista over `store`. One resolver per
/// engine, built in `SimEngineBuilder::start`. `columns` is the same
/// per-table declared column list `fake_row()` reads. A declared table's
/// columns are registered on its first resolve, once the store table and so
/// its id column exist, which makes a declared column orderable and
/// searchable too.
///
/// A name the store has no table for is not-found; the resolver never
/// creates a table.
pub(in crate::sim) fn memory_resolver(
    store: MemoryStore,
    columns: &HashMap<String, Vec<FakerColumn>>,
) -> Arc<dyn VistaResolver> {
    let catalog = MemoryCatalog::new(store.clone());
    let unregistered = Mutex::new(columns.clone());
    Arc::new(move |name: &str| -> Result<Vista> {
        // Registered under the lock, so a concurrent first resolve waits for
        // the metadata instead of building an id-only Vista. Once a name is
        // registered, or was never declared, the catalog alone answers: it
        // gives a store table its id column and reports any other name as
        // not-found.
        let mut unregistered = unregistered.lock().unwrap_or_else(|e| e.into_inner());
        if unregistered.contains_key(name) {
            if !store.table_names().iter().any(|n| n == name) {
                return Err(error!("no table in this store", table = name).mark_not_found());
            }
            let declared = unregistered.remove(name).unwrap_or_default();
            let id_column = store.table(name).id_column().to_string();
            catalog.register(name, table_metadata(&id_column, &declared));
        }
        drop(unregistered);
        catalog.vista(name)
    })
}

/// Every declared column, orderable and searchable, so a sim can
/// `sort`/`search` by what it declared for `fake_row()`. A `where` on an
/// undeclared column is an error once the table declares any column; a
/// table that declares none gets only its id column from the catalog, takes
/// a `where` on any column, but no `sort`.
///
/// A column's type here is a generation hint (a `faker:` generator decides
/// the cell's real form), and generated rows are stored as given, so every
/// column is untyped: sim writes are stored as given too.
fn table_metadata(id_column: &str, declared: &[FakerColumn]) -> VistaMetadata {
    declared.iter().fold(
        VistaMetadata::new().with_id_column(id_column),
        |meta, col| {
            let mut column = Column::untyped(&col.name)
                .with_flag(ColumnFlag::Orderable)
                .with_flag(ColumnFlag::Searchable);
            if col.name == id_column {
                column = column.with_flag(ColumnFlag::Id);
            }
            meta.with_column(column)
        },
    )
}

/// `table()`: the calling sim's own def table, narrowed and resolved like
/// any `table(name)` handle through the engine's own `DataVocab` resolver.
pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("table", || {
        with(|c| Ok(Handle::named(c.kind().def.table.clone())))
    });
}
