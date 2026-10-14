use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_memory::{MemoryCatalog, MemoryStore, MemoryTableShell};
use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::rhai::{DataVocab, Terminals, Writes};
use vantage_vista::{Column, ColumnFlag, Vista, VistaMetadata, VistaResolver};

pub fn rec(pairs: &[(&str, i64)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), CborValue::Integer((*v).into())))
        .collect()
}

/// Table `t`: r1 {a:1, n:3}, r2 {a:2, n:1}, r3 {a:1, n:2}.
pub fn store() -> MemoryStore {
    let store = MemoryStore::new();
    let t = store.table("t");
    t.insert_as("r1", rec(&[("a", 1), ("n", 3)])).unwrap();
    t.insert_as("r2", rec(&[("a", 2), ("n", 1)])).unwrap();
    t.insert_as("r3", rec(&[("a", 1), ("n", 2)])).unwrap();
    store
}

/// Memory Vistas over `store`, declaring `id`, `a`, `n` and `tag`; `mock` a mock
/// without bulk import that already holds `r1 {a:7}`, `auto` and `intid`
/// empty mocks with a numeric id column, flagged `auto` on `auto`.
pub fn resolver(store: &MemoryStore) -> Arc<dyn VistaResolver> {
    let store = store.clone();
    let mock = MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
                .with_id_column("id"),
        )
        .with_record("r1", rec(&[("a", 7)]));
    Arc::new(move |name: &str| {
        if name == "mock" {
            return Ok(Vista::new(name, mock.clone()));
        }
        if name == "auto" || name == "intid" {
            let mut id = Column::new("id", "int").with_flag(ColumnFlag::Id);
            if name == "auto" {
                id = id.with_flag(ColumnFlag::Auto);
            }
            let metadata = VistaMetadata::new().with_column(id).with_id_column("id");
            return Ok(Vista::new(name, MockShell::new().with_metadata(metadata)));
        }
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
            .with_column(Column::new("a", "int"))
            .with_column(Column::new("n", "int").with_flag(ColumnFlag::Orderable))
            .with_column(Column::new("tag", "String"))
            .with_id_column("id");
        let catalog = MemoryCatalog::new(store.clone());
        let shell = MemoryTableShell::new(store.table(name), metadata, catalog);
        Ok(Vista::new(name, shell))
    })
}

/// A host whose `table(name)` goes through `resolver`.
pub fn host_with(resolver: Arc<dyn VistaResolver>, terminals: Terminals) -> Host {
    Host::builder(Limits::background())
        .vocab(DataVocab::new(terminals).with_resolver(resolver))
        .build()
}

pub fn host(store: &MemoryStore, terminals: Terminals) -> Host {
    host_with(resolver(store), terminals)
}

/// Read and write terminals, writes allowed, `list()` uncapped.
pub fn read_write_terminals() -> Terminals {
    Terminals::ReadWrite {
        limit: None,
        writes: Writes::Allowed,
    }
}

/// Read and write terminals whose writes throw "writes aren't available
/// here", with `list()` capped at `limit`.
pub fn read_only_terminals(limit: Option<usize>) -> Terminals {
    Terminals::ReadWrite {
        limit,
        writes: Writes::Denied("writes aren't available here".into()),
    }
}

pub fn read_write(store: &MemoryStore) -> Host {
    host(store, read_write_terminals())
}

pub fn run(host: &Host, script: &str) -> Result<Dynamic, String> {
    host.compile_uncached(&Block::from(script))
        .and_then(|s| s.eval(&Env::new()))
        .map_err(|e| e.to_string())
}

pub fn eval<T: Clone + 'static>(host: &Host, script: &str) -> T {
    let out = run(host, script).unwrap();
    let name = out.type_name();
    out.try_cast::<T>()
        .unwrap_or_else(|| panic!("unexpected result type {name}"))
}

pub fn json(host: &Host, script: &str) -> serde_json::Value {
    vantage_rhai::to_json(&run(host, script).unwrap())
}
