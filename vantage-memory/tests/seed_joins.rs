//! Seeded vistas: seed files load once, and references join integer
//! foreign keys (as seed files usually write them) to text ids.

use std::path::Path;

use ciborium::Value as CborValue;
use vantage_dataset::ReadableValueSet;
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::{Vista, VistaFactory};

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

async fn ids(v: &Vista) -> Vec<String> {
    v.list_values().await.unwrap().keys().cloned().collect()
}

fn client_yaml(seed: &Path) -> String {
    format!(
        "name: client\nid_column: id\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  name: {{ type: string }}\nreferences:\n  orders: {{ table: order, kind: has_many, foreign_key: client_id }}\nmemory:\n  seed: {}\n",
        seed.display()
    )
}

fn order_yaml(seed: &Path) -> String {
    format!(
        "name: order\nid_column: id\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  client_id: {{ type: int }}\nreferences:\n  client: {{ table: client, kind: has_one, foreign_key: client_id }}\nmemory:\n  seed: {}\n",
        seed.display()
    )
}

/// A factory with `client` and `order` seeded from YAML files; orders
/// refer to clients by integer `client_id`.
fn seeded() -> (
    MemoryStore,
    MemoryVistaFactory,
    Vista,
    Vista,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().unwrap();
    let clients = dir.path().join("client.yaml");
    std::fs::write(&clients, "- { id: 1, name: Ann }\n- { id: 2, name: Bob }\n").unwrap();
    let orders = dir.path().join("order.yaml");
    std::fs::write(
        &orders,
        "- { id: o1, client_id: 1 }\n- { id: o2, client_id: 2 }\n- { id: o3, client_id: 1 }\n",
    )
    .unwrap();
    let store = MemoryStore::new();
    let f = MemoryVistaFactory::new(store.clone());
    let client = f.from_yaml(&client_yaml(&clients)).unwrap();
    let order = f.from_yaml(&order_yaml(&orders)).unwrap();
    (store, f, client, order, dir)
}

#[tokio::test]
async fn rebuilding_a_seeded_vista_keeps_edits_and_adds_no_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.yaml");
    std::fs::write(&path, "- { id: n1, body: seeded }\n- { body: no id }\n").unwrap();
    let yaml = format!(
        "name: note\nid_column: id\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  body: {{ type: string }}\nmemory:\n  seed: {}\n",
        path.display()
    );
    let store = MemoryStore::new();
    let f = MemoryVistaFactory::new(store.clone());
    f.from_yaml(&yaml).unwrap();
    let edit: Record<CborValue> = [("body".to_string(), text("edited"))].into_iter().collect();
    store.table("note").patch("n1", &edit);
    let v = f.from_yaml(&yaml).unwrap();
    assert_eq!(ids(&v).await.len(), 2);
    assert_eq!(
        store.table("note").get("n1").unwrap().get("body"),
        Some(&text("edited"))
    );
}

#[tokio::test]
async fn has_many_matches_integer_foreign_keys() {
    let (store, _f, client, _order, _dir) = seeded();
    let ann = store.table("client").get("1").unwrap();
    let orders = client.get_ref("orders", &ann).unwrap();
    assert_eq!(ids(&orders).await, ["o1", "o3"]);
}

#[tokio::test]
async fn has_one_follows_an_integer_foreign_key() {
    let (store, _f, _client, order, _dir) = seeded();
    let o2 = store.table("order").get("o2").unwrap();
    let client = order.get_ref("client", &o2).unwrap();
    assert_eq!(ids(&client).await, ["2"]);
}
