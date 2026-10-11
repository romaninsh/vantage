//! `table(name)`'s resolver over the engine's store, and `table()`'s default
//! to the calling sim's def table.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use vantage_core::{Result, error};
use vantage_memory::vista::Catalog;
use vantage_memory::{MemoryStore, MemoryTableHandle, MemoryTableShell};
use vantage_rhai::rhai::Engine;
use vantage_vista::{Column, ColumnFlag, Handle, TargetResolver, Vista, VistaMetadata};

use super::counted::CountedShell;
use crate::FakerColumn;
use crate::sim::current::with;

/// Resolve a table name to a fresh Vista over `store`, counting its writes
/// into `writes`. One resolver per engine, built in `SimEngineBuilder::start`.
/// `columns` is the same per-table declared column list `fake_row()` reads;
/// reused here so a declared column is also orderable and searchable.
pub(super) fn memory_resolver(
    store: MemoryStore,
    catalog: Catalog,
    columns: Arc<HashMap<String, Vec<FakerColumn>>>,
    writes: Arc<AtomicU64>,
) -> TargetResolver {
    Arc::new(move |name: &str| -> Result<Vista> {
        if !store.table_names().iter().any(|n| n == name) {
            return Err(error!("no table in this store", table = name));
        }
        let shell = catalog.shell(name).unwrap_or_else(|| {
            let table = store.table(name);
            let declared = columns.get(name);
            let metadata = table_metadata(&table, declared);
            let untyped = declared.into_iter().flatten().map(|c| c.name.clone());
            MemoryTableShell::new(table, metadata, catalog.clone()).with_untyped_columns(untyped)
        });
        Ok(Vista::new(name, CountedShell::new(shell, writes.clone())))
    })
}

/// A table with no catalog entry: its id column, plus every declared column
/// as orderable and searchable, so a sim can `sort`/`search` by what it
/// declared for `fake_row()`. A `where` on an undeclared column is an error
/// once the table declares any column; a table that declares none takes a
/// `where` on any column, but no `sort`.
///
/// A column's type here is a generation hint (a `faker:` generator decides
/// the cell's real form), and generated rows are stored as given, so the
/// resolver marks every column untyped: sim writes are stored as given too.
fn table_metadata(table: &MemoryTableHandle, declared: Option<&Vec<FakerColumn>>) -> VistaMetadata {
    let id_column = table.id_column().to_string();
    let mut meta = VistaMetadata::new().with_id_column(id_column.clone());
    for col in declared.into_iter().flatten() {
        let mut column = Column::new(&col.name, &col.ty)
            .with_flag(ColumnFlag::Orderable)
            .with_flag(ColumnFlag::Searchable);
        if col.name == id_column {
            column = column.with_flag(ColumnFlag::Id);
        }
        meta = meta.with_column(column);
    }
    meta
}

/// `table()`: the calling sim's own def table, narrowed and resolved like
/// any `table(name)` handle through the engine's own `DataVocab` resolver.
pub(super) fn register(engine: &mut Engine) {
    engine.register_fn("table", || {
        with(|c| Ok(Handle::named(c.kind().def.table.clone())))
    });
}
