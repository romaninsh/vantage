use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_memory::vista::Catalog;
use vantage_memory::{MemoryStore, MemoryTableShell};
use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_types::Record;
use vantage_vista::mocks::MockShell;
use vantage_vista::{
    Column, DataVocab, TargetResolver, Terminals, Vista, VistaCapabilities, VistaMetadata, Writes,
};

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

/// Memory Vistas over `store`; `nocount` is a mock that can't count.
pub fn resolver(store: &MemoryStore) -> TargetResolver {
    let store = store.clone();
    Arc::new(move |name: &str| {
        if name == "nocount" {
            let caps = VistaCapabilities {
                can_count: false,
                ..VistaCapabilities::default()
            };
            return Ok(Vista::new(
                name,
                Box::new(MockShell::new().with_capabilities(caps)),
            ));
        }
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("n", "int").with_flag("orderable"))
            .with_id_column("id");
        let shell = MemoryTableShell::new(store.table(name), metadata, Catalog::new(store.clone()));
        Ok(Vista::new(name, Box::new(shell)))
    })
}

pub fn host(store: &MemoryStore, terminals: Terminals) -> Host {
    Host::builder(Limits::background())
        .vocab(DataVocab {
            resolver: Some(resolver(store)),
            terminals,
        })
        .build()
}

pub fn read_write(store: &MemoryStore) -> Host {
    host(
        store,
        Terminals::ReadWrite {
            limit: None,
            writes: Writes::Allowed,
        },
    )
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
