//! `lazy:` columns: computed by the Vista on every read, never stored.
#![cfg(feature = "rhai")]

use ciborium::Value as CborValue;
use futures_util::StreamExt;
use vantage_core::VantageError;
use vantage_dataset::{ReadableValueSet, WritableValueSet};
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::{AggregateSpec, FilterOp, SortDirection, Vista, VistaChange, VistaFactory};

const PRODUCT: &str = r#"
name: product
id_column: id
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
references:
  lines: { table: line, kind: has_many, foreign_key: product_ref }
"#;

/// `total` and `big` chain; `product_ref` keys the has-one `product`.
const LINE: &str = r#"
name: line
id_column: id
columns:
  id: { type: string, flags: [id] }
  pnum: { type: int }
  qty: { type: int }
  total: { type: int, lazy: "row.qty * 2" }
  big: { type: bool, lazy: "row.total > 4" }
  product_ref: { type: string, lazy: "\"p\" + row.pnum" }
references:
  product: { table: product, kind: has_one, foreign_key: product_ref }
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

fn setup() -> (MemoryStore, Vista, Vista) {
    let store = MemoryStore::new();
    let f = MemoryVistaFactory::new(store.clone());
    let products = f.from_yaml(PRODUCT).unwrap();
    let lines = f.from_yaml(LINE).unwrap();
    for (id, name) in [("p1", "Tea"), ("p3", "Coffee")] {
        store
            .table("product")
            .upsert(id, record(&[("name", text(name))]));
    }
    for (id, pnum, qty) in [("l1", 3, 1), ("l2", 1, 4)] {
        store
            .table("line")
            .upsert(id, record(&[("pnum", int(pnum)), ("qty", int(qty))]));
    }
    (store, products, lines)
}

#[tokio::test]
async fn reads_fill_computed_columns() {
    let (_s, _p, mut lines) = setup();
    let column = lines.get_column("total").unwrap();
    assert!(column.is_computed());
    assert_eq!(column.expression(), Some("row.qty * 2"));

    let all = lines.list_values().await.unwrap();
    assert_eq!(all["l1"].get("total"), Some(&int(2)));
    assert_eq!(all["l2"].get("big"), Some(&CborValue::Bool(true)));

    let one = lines.get_value("l1").await.unwrap().unwrap();
    assert_eq!(one.get("product_ref"), Some(&text("p3")));

    let (_, some) = lines.get_some_value().await.unwrap().unwrap();
    assert!(some.contains_key("total"));

    let window = lines.fetch_window(1, 1).await.unwrap();
    assert_eq!(window[0].1.get("total"), Some(&int(8)));

    let streamed: Vec<_> = lines.stream_values().collect().await;
    assert!(
        streamed
            .iter()
            .all(|r| r.as_ref().unwrap().1.contains_key("total"))
    );

    lines.set_page_size(1).unwrap();
    let page = lines.fetch_page(1).await.unwrap();
    assert_eq!(page[0].1.get("big"), Some(&CborValue::Bool(false)));
}

#[tokio::test]
async fn watched_changes_carry_computed_columns() {
    let (_s, _p, lines) = setup();
    let mut changes = lines.watch().await.unwrap();
    lines
        .insert_value("l9", &record(&[("pnum", int(1)), ("qty", int(3))]))
        .await
        .unwrap();
    match changes.next().await.unwrap().unwrap() {
        VistaChange::Inserted { value, .. } => assert_eq!(value.get("total"), Some(&int(6))),
        other => panic!("expected an insert, got {other:?}"),
    }
}

#[tokio::test]
async fn an_expression_that_does_not_compile_fails_the_build() {
    let f = MemoryVistaFactory::new(MemoryStore::new());
    let Err(err) = f.from_yaml(&LINE.replace("row.qty * 2", "row.qty *")) else {
        panic!("a broken script must fail the build");
    };
    assert!(names_column(&err, "total"), "{err:?}");
}

#[tokio::test]
async fn writes_drop_computed_columns() {
    let (s, _p, lines) = setup();
    let rec = record(&[("pnum", int(1)), ("qty", int(5)), ("total", int(99))]);
    lines.insert_value("l3", &rec).await.unwrap();
    let stored = s.table("line").get("l3").unwrap();
    assert!(!stored.contains_key("total"), "{stored:?}");
    assert_eq!(
        lines.get_value("l3").await.unwrap().unwrap().get("total"),
        Some(&int(10))
    );

    lines
        .patch_value(
            "l3",
            &record(&[("qty", int(1)), ("big", CborValue::Bool(true))]),
        )
        .await
        .unwrap();
    assert!(!s.table("line").get("l3").unwrap().contains_key("big"));
}

#[tokio::test]
async fn filtering_or_ordering_on_a_computed_column_errors() {
    let (_s, _p, mut lines) = setup();
    let err = lines.add_condition_eq("total", int(2)).unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    assert!(names_column(&err, "total"), "{err:?}");
    let err = lines
        .add_condition("total", FilterOp::Gt, int(2))
        .unwrap_err();
    assert!(names_column(&err, "total"), "{err:?}");
    let err = lines
        .add_order("total", SortDirection::Ascending)
        .unwrap_err();
    assert!(names_column(&err, "total"), "{err:?}");
}

#[tokio::test]
async fn aggregating_over_a_computed_column_errors() {
    let (_s, _p, lines) = setup();
    let Err(err) = lines.aggregate(&AggregateSpec::new("sum", "sum").column("total")) else {
        panic!("summing a computed column must error");
    };
    assert!(names_column(&err, "total"), "{err:?}");
    let Err(err) = lines.aggregate(&AggregateSpec::new("count", "n").group_by("big")) else {
        panic!("grouping by a computed column must error");
    };
    assert!(names_column(&err, "big"), "{err:?}");
}

#[tokio::test]
async fn nested_insert_refuses_a_computed_link() {
    let (s, products, lines) = setup();
    let child = CborValue::Map(vec![(text("name"), text("Cocoa"))]);
    let rec = record(&[("pnum", int(1)), ("qty", int(1)), ("product", child)]);
    let err = lines.insert_value("l7", &rec).await.unwrap_err();
    assert!(names_column(&err, "product_ref"), "{err:?}");

    let lines_payload = CborValue::Array(vec![CborValue::Map(vec![(text("qty"), int(1))])]);
    let rec = record(&[("name", text("Cocoa")), ("lines", lines_payload)]);
    let err = products.insert_value("p7", &rec).await.unwrap_err();
    assert!(names_column(&err, "product_ref"), "{err:?}");
    assert!(s.table("product").get("p7").is_none(), "nothing is written");
}

fn names_column(err: &VantageError, column: &str) -> bool {
    err.context
        .get("column")
        .is_some_and(|v| v.trim_matches('"') == column)
}

#[tokio::test]
async fn has_one_reference_follows_a_computed_key() {
    let (_s, _p, lines) = setup();
    let row = lines.get_value("l1").await.unwrap().unwrap();
    let product = lines.get_ref("product", &row).unwrap();
    let ids: Vec<String> = product.list_values().await.unwrap().into_keys().collect();
    assert_eq!(ids, ["p3"]);
}

#[tokio::test]
async fn has_many_reference_on_a_computed_child_key_errors() {
    let (_s, products, _l) = setup();
    let mut row = products.get_value("p3").await.unwrap().unwrap();
    row.insert("id".into(), text("p3"));
    let Err(err) = products.get_ref("lines", &row) else {
        panic!("has-many on a computed key must not traverse");
    };
    assert!(err.is_unsupported(), "{err}");
    assert!(names_column(&err, "product_ref"), "{err:?}");
}
