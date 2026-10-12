//! The shop every guide example runs against: `client` and `order` memory
//! tables joined both ways, behind a [`VistaResolver`].

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_dataset::ReadableValueSet;
use vantage_memory::{MemoryCatalog, MemoryStore, MemoryTableShell};
use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_types::Record;
use vantage_vista::rhai::{DataVocab, Terminals, Writes, block_on};
use vantage_vista::{Column, ColumnFlag, JoinKey, Reference, Vista, VistaMetadata, VistaResolver};

fn row(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

/// Clients c1 Ada (vip), c2 Ben, c3 Cy (vip). Orders o1 c1 120 paid,
/// o2 c1 40 due, o3 c2 75 paid, o4 c3 15 due.
pub fn shop() -> MemoryStore {
    let store = MemoryStore::new();
    let clients = store.table("client");
    for (id, name, vip) in [
        ("c1", "Ada", true),
        ("c2", "Ben", false),
        ("c3", "Cy", true),
    ] {
        let r = row(&[("name", text(name)), ("vip", CborValue::Bool(vip))]);
        clients.insert_as(id, r).unwrap();
    }
    let orders = store.table("order");
    for (id, client, total, status) in [
        ("o1", "c1", 120, "paid"),
        ("o2", "c1", 40, "due"),
        ("o3", "c2", 75, "paid"),
        ("o4", "c3", 15, "due"),
    ] {
        let r = row(&[
            ("client", text(client)),
            ("total", int(total)),
            ("status", text(status)),
        ]);
        orders.insert_as(id, r).unwrap();
    }
    store
}

fn catalog(store: &MemoryStore) -> MemoryCatalog {
    let catalog = MemoryCatalog::new(store.clone());
    catalog.register(
        "client",
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
            .with_column(Column::new("name", "String").with_flag(ColumnFlag::Orderable))
            .with_column(Column::new("vip", "bool"))
            .with_id_column("id")
            .with_reference(Reference::has_many(
                "orders",
                "order",
                JoinKey::new("id", "client"),
            )),
    );
    catalog.register(
        "order",
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
            .with_column(Column::new("client", "String"))
            .with_column(Column::new("total", "int").with_flag(ColumnFlag::Orderable))
            .with_column(Column::new("status", "String"))
            .with_id_column("id")
            .with_reference(Reference::has_one(
                "client",
                "client",
                JoinKey::to_target_id("client"),
            )),
    );
    catalog
}

/// `table(name)` over the shop: a fresh memory Vista per call. A name the
/// shop doesn't hold gets a new, empty table.
pub fn resolver(store: &MemoryStore) -> Arc<dyn VistaResolver> {
    let catalog = catalog(store);
    Arc::new(move |name: &str| {
        catalog.store().table(name);
        catalog.vista(name)
    })
}

pub fn host(store: &MemoryStore, terminals: Terminals) -> Host {
    Host::builder(Limits::background())
        .vocab(DataVocab::new(terminals).with_resolver(resolver(store)))
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

/// A memory Vista named `name` over `metadata` (id column `id`), in a store
/// of its own holding `rows`.
pub fn memory_vista(
    name: &str,
    metadata: VistaMetadata,
    rows: Vec<(&str, Record<CborValue>)>,
) -> Vista {
    let store = MemoryStore::new();
    for (id, row) in rows {
        store.table(name).insert_as(id, row).unwrap();
    }
    let shell = MemoryTableShell::new(
        store.table(name),
        metadata.with_id_column("id"),
        MemoryCatalog::new(store.clone()),
    );
    Vista::new(name, shell)
}

/// Row `id` of `vista`, which must exist.
pub fn read_one(vista: &Vista, id: &str) -> Record<CborValue> {
    block_on(vista.get_value(id.to_string()))
        .unwrap()
        .unwrap()
        .unwrap()
}

pub fn run(host: &Host, script: &str) -> Result<Dynamic, String> {
    host.compile_uncached(&Block::from(script))
        .and_then(|s| s.eval(&Env::new()))
        .map_err(|e| e.to_string())
}

pub fn json(host: &Host, script: &str) -> serde_json::Value {
    vantage_rhai::to_json(&run(host, script).unwrap())
}
