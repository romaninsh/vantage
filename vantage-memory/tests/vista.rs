use ciborium::Value as CborValue;
use vantage_dataset::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::{FilterOp, SortDirection, Vista, VistaFactory};

const PRODUCT: &str = r#"
name: product
id_column: id
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title, searchable] }
  price: { type: int }
  category: { type: string }
memory:
  indexed: [category]
"#;

const ORDER: &str = r#"
name: order
id_column: id
columns:
  id: { type: string, flags: [id] }
  product: { type: string }
  qty: { type: int }
references:
  product: { table: product, kind: has_one, foreign_key: product }
"#;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}
fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

fn setup() -> (MemoryStore, MemoryVistaFactory) {
    let store = MemoryStore::new();
    let f = MemoryVistaFactory::new(store.clone());
    let p = store.table("product");
    for (id, name, price, cat) in [
        ("p1", "Tea", 3, "drink"),
        ("p2", "Cake", 5, "food"),
        ("p3", "Coffee", 4, "drink"),
    ] {
        p.upsert(
            id,
            record(&[
                ("name", text(name)),
                ("price", int(price)),
                ("category", text(cat)),
            ]),
        );
    }
    (store, f)
}

async fn ids(v: &Vista) -> Vec<String> {
    v.list_values().await.unwrap().keys().cloned().collect()
}

#[tokio::test]
async fn capabilities_are_advertised() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let c = v.capabilities();
    assert!(c.can_count && c.can_insert && c.can_update && c.can_delete && c.can_import);
    assert!(c.can_order && c.can_search && c.can_filter_operators && c.can_subscribe);
    assert!(c.can_set_page_size && c.can_fetch_page && c.can_fetch_window);
    assert!(c.can_traverse_to_record && c.can_traverse_to_set);
    assert!(!c.can_invalidate && !c.can_fetch_next);
}

#[tokio::test]
async fn op_conditions_order_search_and_count() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(PRODUCT).unwrap();
    v.add_condition("price", FilterOp::Gte, int(4)).unwrap();
    v.add_order("price", SortDirection::Descending).unwrap();
    assert_eq!(ids(&v).await, ["p2", "p3"]);
    assert_eq!(v.get_count().await.unwrap(), 2);
    v.add_search("cof").unwrap();
    assert_eq!(ids(&v).await, ["p3"]);
}

#[tokio::test]
async fn add_order_replaces_previous_order() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(PRODUCT).unwrap();
    v.add_order("price", SortDirection::Descending).unwrap();
    v.add_order("name", SortDirection::Ascending).unwrap();
    assert_eq!(ids(&v).await, ["p2", "p3", "p1"]);
}

#[tokio::test]
async fn pages_and_windows() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(PRODUCT).unwrap();
    v.add_order("price", SortDirection::Ascending).unwrap();
    v.set_page_size(2).unwrap();
    let p2: Vec<String> = v
        .fetch_page(2)
        .await
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(p2, ["p2"]);
    assert!(v.fetch_page(0).await.is_err());
    let w: Vec<String> = v
        .fetch_window(1, 1)
        .await
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(w, ["p3"]);
    assert_eq!(
        ids(&v).await.len(),
        3,
        "a window must not narrow the vista itself"
    );
}

#[tokio::test]
async fn writes_through_the_vista_reach_the_store() {
    let (s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let rec = record(&[("name", text("Scone")), ("price", int(2))]);
    let id = v.insert_return_id_value(&rec).await.unwrap();
    assert_eq!(id, "1");
    assert_eq!(s.table("product").len(), 4);
    v.patch_value(&id, &record(&[("price", int(9))]))
        .await
        .unwrap();
    assert_eq!(
        s.table("product").get(&id).unwrap().get("price"),
        Some(&int(9))
    );
    v.delete(&id).await.unwrap();
    assert!(v.delete(&id).await.is_err());
}

#[tokio::test]
async fn clone_shell_isolates_query_state() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let mut narrowed = Vista::new(v.name(), v.source.clone_shell().unwrap());
    narrowed.add_condition_eq("category", text("food")).unwrap();
    assert_eq!(ids(&narrowed).await, ["p2"]);
    assert_eq!(ids(&v).await.len(), 3);
}

#[tokio::test]
async fn has_one_reference_traverses_to_the_target_row() {
    let (s, f) = setup();
    let _products = f.from_yaml(PRODUCT).unwrap();
    let orders = f.from_yaml(ORDER).unwrap();
    s.table("order")
        .upsert("o1", record(&[("product", text("p3")), ("qty", int(2))]));
    let row = s.table("order").get("o1").unwrap();
    let target = orders.get_ref("product", &row).unwrap();
    assert_eq!(ids(&target).await, ["p3"]);
    assert!(
        target.get_column("price").is_some(),
        "target uses the catalog's metadata"
    );
}

#[tokio::test]
async fn seed_file_is_loaded_by_the_factory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cat.yaml");
    std::fs::write(&path, "- { id: c1, name: Cat }\n").unwrap();
    let store = MemoryStore::new();
    let f = MemoryVistaFactory::new(store.clone());
    let yaml = format!(
        "name: cat\nid_column: id\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  name: {{ type: string }}\nmemory:\n  seed: {}\n",
        path.display()
    );
    let v = f.from_yaml(&yaml).unwrap();
    assert_eq!(ids(&v).await, ["c1"]);
}

#[tokio::test]
async fn spec_indexes_apply_to_a_pre_existing_table() {
    let (s, f) = setup();
    assert!(!s.table("product").is_indexed("category"));
    f.from_yaml(PRODUCT).unwrap();
    assert!(s.table("product").is_indexed("category"));
}

#[tokio::test]
async fn id_column_mismatch_with_existing_table_errors() {
    let (_s, f) = setup();
    let yaml = PRODUCT
        .replace("id_column: id", "id_column: sku")
        .replace("  id: {", "  sku: {");
    assert!(f.from_yaml(&yaml).is_err());
}

#[tokio::test]
async fn computed_columns_are_rejected() {
    let (_s, f) = setup();
    let lazy = PRODUCT.replace("price: { type: int }", "price: { type: int, lazy: \"1\" }");
    assert!(f.from_yaml(&lazy).is_err());
    let expr = PRODUCT.replace("price: { type: int }", "price: { type: int, expr: \"1\" }");
    assert!(f.from_yaml(&expr).is_err());
}
