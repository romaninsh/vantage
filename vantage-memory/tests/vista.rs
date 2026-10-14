use ciborium::Value as CborValue;
use vantage_dataset::{ReadableValueSet, WritableValueSet};
use vantage_memory::{MemoryCatalog, MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::{
    Column, FilterOp, JoinKey, Reference, SortDirection, Vista, VistaFactory, VistaMetadata,
    VistaResolver,
};

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
async fn a_condition_on_an_undeclared_column_is_an_error() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let err = v
        .with_condition("colour", FilterOp::Eq, text("red"))
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err}");
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
async fn a_text_operand_is_conformed_to_an_integer_column() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let equal = v
        .clone()
        .with_condition("price", FilterOp::Eq, text("5"))
        .unwrap();
    assert_eq!(ids(&equal).await, ["p2"]);
    let above = v.with_condition("price", FilterOp::Gt, text("3")).unwrap();
    assert_eq!(ids(&above).await, ["p2", "p3"]);
}

#[test]
fn an_unknown_relation_is_not_found() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let err = v.get_ref("no_such_relation", None).unwrap_err();
    assert!(err.is_not_found(), "{err:?}");
}

#[test]
fn a_contained_relation_is_refused() {
    let store = MemoryStore::new();
    store.table("client");
    let catalog = MemoryCatalog::new(store);
    catalog.register(
        "client",
        VistaMetadata::new().with_reference(Reference::contains_many("tags", "tags")),
    );
    let err = catalog
        .vista("client")
        .unwrap()
        .get_ref("tags", None)
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
}

#[test]
fn a_relation_to_a_table_the_store_lacks_is_not_found() {
    let store = MemoryStore::new();
    store.table("client");
    let catalog = MemoryCatalog::new(store);
    catalog.register(
        "client",
        VistaMetadata::new().with_reference(Reference::has_many(
            "orders",
            "order",
            JoinKey::new("id", "client"),
        )),
    );
    let err = catalog
        .vista("client")
        .unwrap()
        .get_ref("orders", None)
        .unwrap_err();
    assert!(err.is_not_found(), "{err}");
}

#[test]
fn a_registered_name_carries_its_metadata_and_gets_a_table() {
    let store = MemoryStore::new();
    let catalog = MemoryCatalog::new(store.clone());
    catalog.register(
        "client",
        VistaMetadata::new()
            .with_id_column("id")
            .with_column(Column::new("name", "string")),
    );
    let vista = catalog.vista("client").unwrap();
    assert_eq!(vista.name(), "client");
    assert!(vista.column("name").is_some());
    assert_eq!(store.table_names(), ["client"]);
}

#[test]
fn an_unregistered_store_table_gets_only_its_id_column() {
    let store = MemoryStore::new();
    store.table("order");
    let vista = MemoryCatalog::new(store).vista("order").unwrap();
    assert_eq!(vista.id_column(), Some("id"));
    assert!(vista.columns().is_empty());
}

#[test]
fn an_unknown_name_is_not_found_and_creates_no_table() {
    let store = MemoryStore::new();
    let err = MemoryCatalog::new(store.clone()).vista("nope").unwrap_err();
    assert!(err.is_not_found(), "{err}");
    assert!(store.table_names().is_empty());
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
