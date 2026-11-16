use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::error;
use vantage_dataset::{ReadableValueSet, WritableValueSet};
use vantage_memory::{AnyMemoryType, MemoryDB, MemoryStore, MemoryVistaFactory, MemoryVistaSpec};
use vantage_table::table::{BuildKind, Hook, Phase, Table, TableInjector, before_hook_sync};
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{FilterOp, SortDirection, Transaction, Vista, VistaFactory};

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

#[tokio::test]
async fn add_order_breaks_ties_and_set_order_drops_them() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(PRODUCT).unwrap();
    v.set_order(Some(("category", SortDirection::Ascending)))
        .unwrap();
    v.add_order("price", SortDirection::Descending).unwrap();
    // p1 (3) and p3 (4) are both drinks; price breaks the tie.
    assert_eq!(ids(&v).await, ["p3", "p1", "p2"]);
    v.set_order(Some(("category", SortDirection::Ascending)))
        .unwrap();
    assert!(v.query().then_by.is_empty());
    assert_eq!(ids(&v).await, ["p1", "p3", "p2"]);
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
    let table = Table::<MemoryDB, EmptyEntity>::new("client", MemoryDB::new())
        .with_id_column("id")
        .with_contained_many("tags", "tags", |db| Table::new("tag", db), None);
    let vista = MemoryVistaFactory::new(MemoryStore::new())
        .from_table(table)
        .unwrap();
    let err = vista.get_ref("tags", None).unwrap_err();
    assert!(err.is_unsupported(), "{err}");
}

const CLIENT: &str = r#"
name: client
id_column: id
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
references:
  orders: { table: order, kind: has_many, foreign_key: client }
"#;

#[test]
fn a_relation_to_a_table_the_store_lacks_is_an_error_and_creates_no_table() {
    let store = MemoryStore::new();
    let client = MemoryVistaFactory::new(store.clone())
        .from_yaml(CLIENT)
        .unwrap();
    assert!(client.get_ref("orders", None).is_err());
    assert_eq!(store.table_names(), ["client"]);
}

#[test]
fn a_spec_vista_carries_its_columns_and_gets_a_table() {
    let store = MemoryStore::new();
    let vista = MemoryVistaFactory::new(store.clone())
        .from_yaml(CLIENT)
        .unwrap();
    assert_eq!(vista.name(), "client");
    assert!(vista.column("name").is_some());
    assert_eq!(store.table_names(), ["client"]);
}

#[test]
fn a_bare_table_gets_only_the_store_id_column() {
    let store = MemoryStore::new();
    store.table("order");
    let table = Table::<MemoryDB, EmptyEntity>::new("order", MemoryDB::from_store(store.clone()));
    let vista = MemoryVistaFactory::new(store).from_table(table).unwrap();
    assert_eq!(vista.id_column(), Some("id"));
    assert!(vista.columns().is_empty());
}

fn upper(record: &mut Record<AnyMemoryType>, field: &str) {
    if let Some(CborValue::Text(t)) = record.get(field).map(|v| v.value().clone()) {
        let upper = AnyMemoryType::untyped(text(&t.to_uppercase()));
        record.insert(field.to_string(), upper);
    }
}

fn upper_hook(field: &'static str) -> Hook<MemoryDB> {
    Hook::BeforeSave(
        Phase::Populate,
        before_hook_sync::<MemoryDB>(move |record, _| {
            upper(record, field);
            Ok(())
        }),
    )
}

#[tokio::test]
async fn an_injected_hook_runs_on_a_shell_write() {
    let (s, f) = setup();
    let injector: TableInjector<MemoryDB> = Arc::new(|_, table| {
        table.add_hook(upper_hook("name"));
        Ok(())
    });
    let v = f.with_injector(injector).from_yaml(PRODUCT).unwrap();
    v.insert_value("p9", &record(&[("name", text("tea"))]))
        .await
        .unwrap();
    let stored = s.table("product").get("p9").unwrap();
    assert_eq!(stored.get("name"), Some(&text("TEA")));
}

#[tokio::test]
async fn an_injected_hook_can_refuse_a_write() {
    let (s, f) = setup();
    let injector: TableInjector<MemoryDB> = Arc::new(|_, table| {
        table.add_hook(Hook::BeforeSave(
            Phase::Validate,
            before_hook_sync::<MemoryDB>(|_, _| Err(error!("closed for writes"))),
        ));
        Ok(())
    });
    let v = f.with_injector(injector).from_yaml(PRODUCT).unwrap();
    let err = v
        .insert_value("p9", &record(&[("name", text("tea"))]))
        .await
        .unwrap_err();
    assert_eq!(err.message(), "closed for writes");
    assert!(s.table("product").get("p9").is_none());
}

#[tokio::test]
async fn an_import_refused_by_a_hook_writes_nothing() {
    let (s, f) = setup();
    let injector: TableInjector<MemoryDB> = Arc::new(|_, table| {
        table.add_hook(Hook::BeforeSave(
            Phase::Validate,
            before_hook_sync::<MemoryDB>(|record, _| {
                match record.get("name").map(|v| v.value().clone()) {
                    Some(CborValue::Text(name)) if name == "bad" => Err(error!("bad name")),
                    _ => Ok(()),
                }
            }),
        ));
        Ok(())
    });
    let v = f.with_injector(injector).from_yaml(PRODUCT).unwrap();
    let rows: indexmap::IndexMap<String, Record<CborValue>> = [
        ("n1".to_string(), record(&[("name", text("good"))])),
        ("n2".to_string(), record(&[("name", text("bad"))])),
    ]
    .into_iter()
    .collect();
    assert!(v.import_values(&rows).await.is_err());
    let product = s.table("product");
    assert!(product.get("n1").is_none() && product.get("n2").is_none());
}

#[tokio::test]
async fn an_import_outside_a_transaction_does_not_wait_on_an_open_one() {
    let (s, f) = setup();
    let product = f.from_yaml(PRODUCT).unwrap();
    let stock = f
        .from_yaml("name: stock\ncolumns:\n  id: { flags: [id] }\n  qty: {}\n")
        .unwrap();
    let tx = Transaction::new();
    tx.wrap(&product)
        .insert_value("p9", &record(&[("name", text("Juice"))]))
        .await
        .unwrap();
    let rows: indexmap::IndexMap<String, Record<CborValue>> = [
        ("a".to_string(), record(&[("qty", int(1))])),
        ("b".to_string(), record(&[("qty", int(2))])),
    ]
    .into_iter()
    .collect();
    let imported = tokio::time::timeout(Duration::from_millis(500), stock.import_values(&rows))
        .await
        .expect("the import waited on the open transaction");
    assert_eq!(imported.unwrap(), 2);
    tx.commit().await.unwrap();
    assert!(s.table("stock").get("b").is_some());
    assert!(s.table("product").get("p9").is_some());
}

#[test]
fn a_resolver_miss_is_an_error() {
    let store = MemoryStore::new();
    store.table("order");
    let client: MemoryVistaSpec = serde_yaml_ng::from_str(CLIENT).unwrap();
    let f = MemoryVistaFactory::new(store).with_resolver(Arc::new(move |name: &str| {
        (name == "client").then(|| client.clone())
    }));
    let err = f
        .from_yaml(CLIENT)
        .unwrap()
        .get_ref("orders", None)
        .unwrap_err();
    let shown = format!("{err:?}");
    assert!(shown.contains("reference target has no spec"), "{shown}");
    assert!(shown.contains("order"), "{shown}");
}

#[tokio::test]
async fn a_traversed_target_keeps_its_spec_types() {
    let (s, f) = setup();
    let product = PRODUCT.replace("  category:", "  rating: { type: decimal }\n  category:");
    f.from_yaml(&product).unwrap();
    let orders = f.from_yaml(ORDER).unwrap();
    s.table("order")
        .upsert("o1", record(&[("product", text("p3"))]));
    let row = s.table("order").get("o1").unwrap();
    let target = orders.get_ref("product", Some(&row)).unwrap();
    let ty = |name: &str| target.column(name).unwrap().original_type.clone();
    assert_eq!(ty("price").as_deref(), Some("int"));
    assert_eq!(ty("rating").as_deref(), Some("decimal"));
}

const CLIENT_ORDER: &str = r#"
name: order
id_column: id
columns:
  id: { type: string, flags: [id] }
  client: { type: string }
  note: { type: string }
"#;

#[tokio::test]
async fn a_reference_target_is_built_by_the_factory() {
    let store = MemoryStore::new();
    let specs: HashMap<String, MemoryVistaSpec> = [CLIENT, CLIENT_ORDER]
        .into_iter()
        .map(|yaml| serde_yaml_ng::from_str::<MemoryVistaSpec>(yaml).unwrap())
        .map(|spec| (spec.name.clone(), spec))
        .collect();
    let builds = Arc::new(Mutex::new(Vec::new()));
    let seen = builds.clone();
    let injector: TableInjector<MemoryDB> = Arc::new(move |cx, table| {
        seen.lock()
            .unwrap()
            .push((cx.key.to_string(), cx.kind.clone()));
        if cx.key == "order" {
            table.add_hook(upper_hook("note"));
        }
        Ok(())
    });
    let f = MemoryVistaFactory::new(store.clone())
        .with_resolver(Arc::new(move |name: &str| specs.get(name).cloned()))
        .with_injector(injector);
    let client = f.from_yaml(CLIENT).unwrap();
    client
        .insert_value("c1", &record(&[("name", text("Ann"))]))
        .await
        .unwrap();
    let row = store.table("client").get("c1").unwrap();

    let orders = client.get_ref("orders", Some(&row)).unwrap();
    orders
        .insert_value("o1", &record(&[("note", text("rush"))]))
        .await
        .unwrap();

    let reference = BuildKind::Reference {
        from: "client".into(),
        relation: "orders".into(),
    };
    assert!(
        builds
            .lock()
            .unwrap()
            .contains(&("order".to_string(), reference)),
        "{:?}",
        builds.lock().unwrap()
    );
    let stored = store.table("order").get("o1").unwrap();
    assert_eq!(stored.get("note"), Some(&text("RUSH")));
    assert_eq!(stored.get("client"), Some(&text("c1")));
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

const LOAN: &str = r#"
name: loan
id_column: id
columns:
  id: { type: string, flags: [id] }
  due: { type: date }
"#;

#[tokio::test]
async fn a_date_column_stores_one_form_from_any_write() {
    let (_s, f) = setup();
    let v = f.from_yaml(LOAN).unwrap();
    let day = CborValue::Tag(100, Box::new(text("2026-12-16")));
    let rfc = |s: &str| CborValue::Tag(0, Box::new(text(s)));

    v.insert_value("l1", &record(&[("due", text("2026-12-16"))]))
        .await
        .unwrap();
    v.insert_value("l2", &record(&[("due", text("2026-12-16T00:00:00+00:00"))]))
        .await
        .unwrap();
    v.insert_value("l3", &record(&[("due", rfc("2026-12-16T00:00:00Z"))]))
        .await
        .unwrap();
    v.insert_value("l4", &record(&[("due", day.clone())]))
        .await
        .unwrap();
    v.insert_value("l5", &record(&[("due", text("2026-01-01"))]))
        .await
        .unwrap();
    v.patch_value("l5", &record(&[("due", text("2026-12-16T00:00:00.000Z"))]))
        .await
        .unwrap();

    for (id, row) in v.list_values().await.unwrap() {
        assert_eq!(row["due"], day, "{id}");
    }
    let by_text = v
        .with_condition("due", FilterOp::Eq, text("2026-12-16T00:00:00Z"))
        .unwrap();
    assert_eq!(ids(&by_text).await.len(), 5);
}

#[tokio::test]
async fn a_date_column_matches_quicksearch() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(LOAN).unwrap();
    v.insert_value("l1", &record(&[("due", text("2026-12-16"))]))
        .await
        .unwrap();
    v.insert_value("l2", &record(&[("due", text("2027-01-05"))]))
        .await
        .unwrap();
    v.set_search(Some("2026-12")).unwrap();
    assert_eq!(ids(&v).await, ["l1"]);
    let like = f
        .from_yaml(LOAN)
        .unwrap()
        .with_condition("due", FilterOp::Like, text("%01-05"))
        .unwrap();
    assert_eq!(ids(&like).await, ["l2"]);
}
