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

/// The `order` vista, with `client_id` declared as `client_id_spec`.
fn order_yaml(seed: &Path, client_id_spec: &str) -> String {
    format!(
        "name: order\nid_column: id\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  client_id: {client_id_spec}\nreferences:\n  client: {{ table: client, kind: has_one, foreign_key: client_id }}\nmemory:\n  seed: {}\n",
        seed.display()
    )
}

const CLIENT_ROWS: &str = "- { id: 1, name: Ann }\n- { id: 2, name: Bob }\n";
const ORDER_ROWS: &str =
    "- { id: o1, client_id: 1 }\n- { id: o2, client_id: 2 }\n- { id: o3, client_id: 1 }\n";

/// A factory with `client` and `order` seeded from YAML files; orders
/// refer to clients by integer `client_id` cells, declared `client_id_spec`.
fn seeded_as(
    client_id_spec: &str,
) -> (
    MemoryStore,
    MemoryVistaFactory,
    Vista,
    Vista,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().unwrap();
    let clients = dir.path().join("client.yaml");
    std::fs::write(&clients, CLIENT_ROWS).unwrap();
    let orders = dir.path().join("order.yaml");
    std::fs::write(&orders, ORDER_ROWS).unwrap();
    let store = MemoryStore::new();
    let f = MemoryVistaFactory::new(store.clone());
    let client = f.from_yaml(&client_yaml(&clients)).unwrap();
    let order = f.from_yaml(&order_yaml(&orders, client_id_spec)).unwrap();
    (store, f, client, order, dir)
}

/// Both directions of the client/order join hold for `client_id_spec`.
async fn assert_joins(client_id_spec: &str) {
    let (store, _f, client, order, _dir) = seeded_as(client_id_spec);
    let ann = store.table("client").get("1").unwrap();
    let orders = client.get_ref("orders", Some(&ann)).unwrap();
    assert_eq!(
        ids(&orders).await,
        ["o1", "o3"],
        "has_many, {client_id_spec}"
    );
    let o2 = store.table("order").get("o2").unwrap();
    let parent = order.get_ref("client", Some(&o2)).unwrap();
    assert_eq!(ids(&parent).await, ["2"], "has_one, {client_id_spec}");
}

#[tokio::test]
async fn typed_foreign_keys_seeded_with_integers_join() {
    assert_joins("{ type: int }").await;
    assert_joins("{ type: string }").await;
}

/// With no `type:`, the cells stay integers and the has-many condition
/// (text, from the client's id) matches through the both-forms fallback.
#[tokio::test]
async fn an_untyped_foreign_key_seeded_with_integers_joins() {
    let (store, ..) = seeded_as("{}");
    let o1 = store.table("order").get("o1").unwrap();
    assert_eq!(o1.get("client_id"), Some(&CborValue::Integer(1.into())));
    assert_joins("{}").await;
}

/// A reference with `keys:` narrows the target by every pair.
#[tokio::test]
async fn a_multi_key_reference_matches_every_key() {
    let store = MemoryStore::new();
    let rows: Vec<serde_json::Value> = serde_yaml_ng::from_str(
        "- { id: d1, product: p1, version: v1 }\n- { id: d2, product: p1, version: v2 }\n\
         - { id: d3, product: p2, version: v1 }\n",
    )
    .unwrap();
    vantage_memory::seed::load(&store.table("deployment"), rows).unwrap();
    let f = MemoryVistaFactory::new(store.clone());
    let release = f
        .from_yaml(
            "name: release\nid_column: id\ncolumns:\n  id: { flags: [id] }\n  product_id: {}\n  \
             version_id: {}\nreferences:\n  deployments: { table: deployment, kind: has_many, \
             keys: [{ from: product_id, to: product }, { from: version_id, to: version }] }\n",
        )
        .unwrap();
    let row: Record<CborValue> = [
        ("id".to_string(), text("r1")),
        ("product_id".to_string(), text("p1")),
        ("version_id".to_string(), text("v1")),
    ]
    .into_iter()
    .collect();
    let deployments = release.get_ref("deployments", Some(&row)).unwrap();
    assert_eq!(ids(&deployments).await, ["d1"]);
}
