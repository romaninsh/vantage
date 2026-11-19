//! The shop every guide example runs against: `client` and `order` memory
//! tables joined both ways, behind a [`VistaResolver`].

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_dataset::ReadableValueSet;
use vantage_memory::{MemoryStore, MemoryVistaFactory, MemoryVistaSpec};
use vantage_rhai::rhai::Dynamic;
use vantage_rhai::{Block, Env, Host, Limits};
use vantage_types::Record;
use vantage_vista::rhai::{DataVocab, Terminals, Writes, block_on};
use vantage_vista::{ColumnSpec, Vista, VistaFactory, VistaMetadata, VistaResolver};

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

const CLIENT: &str = r#"
name: client
id_column: id
columns:
  id: { type: String, flags: [id] }
  name: { type: String }
  vip: { type: bool }
references:
  orders: { table: order, kind: has_many, foreign_key: client }
"#;

const ORDER: &str = r#"
name: order
id_column: id
columns:
  id: { type: String, flags: [id] }
  client: { type: String }
  total: { type: int }
  status: { type: String }
references:
  client: { table: client, kind: has_one, foreign_key: client }
"#;

/// The import target for orders, copied as they are or mapped. Its columns
/// are untyped, so imported cells are stored as given.
const ARCHIVE: &str = r#"
name: archive
id_column: id
columns:
  id: { type: String, flags: [id] }
  client: {}
  total: {}
  status: {}
  amount: {}
  note: {}
"#;

/// The import target for clients.
const BACKUP: &str = r#"
name: backup
id_column: id
columns:
  id: { type: String, flags: [id] }
  name: {}
  vip: {}
"#;

/// `table(name)` over the shop: a fresh memory Vista per call. A name the
/// shop doesn't hold gets a new, empty table.
pub fn resolver(store: &MemoryStore) -> Arc<dyn VistaResolver> {
    let factory = MemoryVistaFactory::new(store.clone());
    // Built once up front, so a traversal to either finds its spec.
    factory.from_yaml(CLIENT).unwrap();
    factory.from_yaml(ORDER).unwrap();
    Arc::new(move |name: &str| match name {
        "client" => factory.from_yaml(CLIENT),
        "order" => factory.from_yaml(ORDER),
        "archive" => factory.from_yaml(ARCHIVE),
        "backup" => factory.from_yaml(BACKUP),
        other => factory.build_from_spec(MemoryVistaSpec::new(other)),
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

/// A memory Vista named `name` declaring `metadata`'s columns (id column
/// `id`, a computed column as `lazy:`), in a store of its own holding `rows`.
pub fn memory_vista(
    name: &str,
    metadata: VistaMetadata,
    rows: Vec<(&str, Record<CborValue>)>,
) -> Vista {
    let store = MemoryStore::new();
    for (id, row) in rows {
        store.table(name).insert_as(id, row).unwrap();
    }
    let mut spec = MemoryVistaSpec::new(name);
    spec.id_column = Some("id".to_string());
    for (column_name, column) in &metadata.columns {
        let mut col = ColumnSpec::new();
        col.col_type = column.original_type.clone();
        col.lazy = column.expression().map(str::to_string);
        spec.columns.insert(column_name.clone(), col);
    }
    MemoryVistaFactory::new(store)
        .build_from_spec(spec)
        .unwrap()
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
