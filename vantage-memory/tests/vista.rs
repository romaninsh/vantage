use ciborium::Value as CborValue;
use vantage_dataset::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::{Capability, FilterOp, SortDirection, Vista, VistaFactory};

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
    for cap in [
        Capability::Insert,
        Capability::Update,
        Capability::Delete,
        Capability::Import,
        Capability::Subscribe,
        Capability::Order,
        Capability::Search,
        Capability::FilterOps,
        Capability::Window,
        Capability::Traverse,
        Capability::ConfineWrites,
    ] {
        assert!(c.has(cap), "{cap} is not advertised");
    }
    assert!(!c.has(Capability::Cursor) && !c.has(Capability::NativeConditions));
}

#[tokio::test]
async fn a_condition_on_an_undeclared_column_is_an_error() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let err = v
        .with_condition("colour", FilterOp::Eq, text("red"))
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err}");
}

#[tokio::test]
async fn equality_conforms_text_to_an_integer_column() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let v = v.with_condition("price", FilterOp::Eq, text("5")).unwrap();
    assert_eq!(ids(&v).await, ["p2"]);
}

#[tokio::test]
async fn a_range_conforms_text_to_an_integer_column() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let v = v.with_condition("price", FilterOp::Gt, text("3")).unwrap();
    assert_eq!(ids(&v).await, ["p2", "p3"]);
}

/// `qty` has no `type:`, so a written integer stays an integer and ranges
/// and order compare numbers, not text.
#[tokio::test]
async fn an_untyped_column_keeps_written_integers() {
    let (s, f) = setup();
    let yaml = "name: stock\nid_column: id\ncolumns:\n  id: { flags: [id] }\n  qty: {}\n";
    let v = f.from_yaml(yaml).unwrap();
    for (id, qty) in [("a", 9), ("b", 10), ("c", 100)] {
        v.replace_value(id, &record(&[("qty", int(qty))]))
            .await
            .unwrap();
    }
    let stored = s.table("stock").get("b").unwrap();
    assert_eq!(stored.get("qty"), Some(&int(10)));
    let mut over = v.with_condition("qty", FilterOp::Gt, int(9)).unwrap();
    over.set_order(Some(("qty", SortDirection::Descending)))
        .unwrap();
    assert_eq!(ids(&over).await, ["c", "b"]);
}

#[tokio::test]
async fn op_conditions_order_search_and_count() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(PRODUCT).unwrap();
    v.add_condition("price", FilterOp::Gte, int(4)).unwrap();
    v.set_order(Some(("price", SortDirection::Descending)))
        .unwrap();
    assert_eq!(ids(&v).await, ["p2", "p3"]);
    assert_eq!(v.count().await.unwrap(), 2);
    v.set_search(Some("cof")).unwrap();
    assert_eq!(ids(&v).await, ["p3"]);
}

#[test]
fn an_unknown_relation_is_not_found() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let err = v.get_ref("no_such_relation", None).unwrap_err();
    assert!(err.is_not_found(), "{err:?}");
}

fn datetime(s: &str) -> CborValue {
    CborValue::Tag(0, Box::new(text(s)))
}

/// A datetime operand (`Tag(0, rfc3339)`) compares as text with dates stored
/// as text and with stored datetimes.
#[tokio::test]
async fn datetime_operand_orders_against_text_dates() {
    let store = MemoryStore::new();
    let f = MemoryVistaFactory::new(store.clone());
    let events = store.table("event");
    events.upsert(
        "old",
        record(&[("updated_at", text("2026-10-01T00:00:00Z"))]),
    );
    events.upsert(
        "mid",
        record(&[("updated_at", text("2026-10-05T00:00:00Z"))]),
    );
    events.upsert(
        "new",
        record(&[("updated_at", datetime("2026-10-09T00:00:00Z"))]),
    );
    let yaml = "name: event\nid_column: id\ncolumns:\n  id: { type: string, flags: [id] }\n  updated_at: { type: string }\n";

    for (op, expected) in [
        (FilterOp::Gt, "new"),
        (FilterOp::Lt, "old"),
        (FilterOp::Eq, "mid"),
    ] {
        let mut v = f.from_yaml(yaml).unwrap();
        v.add_condition("updated_at", op, datetime("2026-10-05T00:00:00Z"))
            .unwrap();
        assert_eq!(ids(&v).await, [expected], "{op:?}");
    }
}

#[tokio::test]
async fn set_order_replaces_previous_order() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(PRODUCT).unwrap();
    v.set_order(Some(("price", SortDirection::Descending)))
        .unwrap();
    v.set_order(Some(("name", SortDirection::Ascending)))
        .unwrap();
    assert_eq!(ids(&v).await, ["p2", "p3", "p1"]);
}

#[tokio::test]
async fn windows_carry_the_total() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(PRODUCT).unwrap();
    v.set_order(Some(("price", SortDirection::Ascending)))
        .unwrap();
    let page = v.fetch_window(2, 2).await.unwrap();
    let ids_of = |page: &vantage_vista::Page| -> Vec<String> {
        page.rows.iter().map(|(id, _)| id.clone()).collect()
    };
    assert_eq!(ids_of(&page), ["p2"]);
    assert_eq!(page.total, Some(3));
    let w = v.fetch_window(1, 1).await.unwrap();
    assert_eq!(ids_of(&w), ["p3"]);
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
    v.delete(&id).await.unwrap();
}

#[tokio::test]
async fn upsert_inserts_then_replaces() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();

    v.upsert_value("new1", &record(&[("name", text("Bun"))]))
        .await
        .unwrap();
    let row = v.get_value("new1").await.unwrap().unwrap();
    assert_eq!(row.get("name"), Some(&text("Bun")));

    v.upsert_value("new1", &record(&[("name", text("Bun v2"))]))
        .await
        .unwrap();
    let row = v.get_value("new1").await.unwrap().unwrap();
    assert_eq!(row.get("name"), Some(&text("Bun v2")));
}

#[tokio::test]
async fn patch_of_a_missing_row_is_not_found_and_delete_is_ok() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let err = v
        .patch_value("nope", &record(&[("name", text("x"))]))
        .await
        .unwrap_err();
    assert!(err.is_not_found(), "{err}");
    v.delete("nope").await.unwrap();
}

#[tokio::test]
async fn a_clone_isolates_query_state() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let narrowed = v
        .clone()
        .with_condition("category", FilterOp::Eq, text("food"))
        .unwrap();
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
    let target = orders.get_ref("product", Some(&row)).unwrap();
    assert_eq!(ids(&target).await, ["p3"]);
    assert!(target.is_narrowed(), "the join is recorded on the target");
    assert!(
        target.column("price").is_some(),
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
async fn expr_columns_are_rejected() {
    let (_s, f) = setup();
    let expr = PRODUCT.replace("price: { type: int }", "price: { type: int, expr: \"1\" }");
    assert!(f.from_yaml(&expr).is_err());
}
