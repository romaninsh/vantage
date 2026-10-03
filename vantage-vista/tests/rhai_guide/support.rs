//! The shop every guide example runs against: `client` and `order` memory
//! tables joined both ways, behind a [`TargetResolver`].

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_memory::vista::Catalog;
use vantage_memory::{MemoryStore, MemoryTableShell};
use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_types::Record;
use vantage_vista::{
    Column, DataVocab, Reference, ReferenceKind, TargetResolver, Terminals, Vista, VistaMetadata,
    Writes,
};

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

fn catalog(store: &MemoryStore) -> Catalog {
    let catalog = Catalog::new(store.clone());
    catalog.register(
        "client",
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("name", "String").with_flag("orderable"))
            .with_column(Column::new("vip", "bool"))
            .with_id_column("id")
            .with_reference(Reference::new(
                "orders",
                "order",
                ReferenceKind::HasMany,
                "client",
            )),
    );
    catalog.register(
        "order",
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("client", "String"))
            .with_column(Column::new("total", "int").with_flag("orderable"))
            .with_column(Column::new("status", "String"))
            .with_id_column("id")
            .with_reference(Reference::new(
                "client",
                "client",
                ReferenceKind::HasOne,
                "client",
            )),
    );
    catalog
}

/// `table(name)` over the shop: a fresh memory Vista per call.
pub fn resolver(store: &MemoryStore) -> TargetResolver {
    let catalog = catalog(store);
    Arc::new(move |name: &str| {
        let metadata = catalog
            .get(name)
            .unwrap_or_else(|| VistaMetadata::new().with_id_column("id"));
        let table = catalog.store().table(name);
        let shell = MemoryTableShell::new(table, metadata, catalog.clone());
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

pub fn json(host: &Host, script: &str) -> serde_json::Value {
    vantage_rhai::to_json(&run(host, script).unwrap())
}
