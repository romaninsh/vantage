//! `lazy:` columns: computed by the Vista on every read, never stored.
#![cfg(feature = "rhai")]

use ciborium::Value as CborValue;
use vantage_core::VantageError;
use vantage_dataset::{ReadableValueSet, WritableValueSet};
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_types::Record;
use vantage_vista::VistaFactory;

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

#[tokio::test]
async fn lazy_columns_are_computed_on_read_and_never_stored() {
    let store = MemoryStore::new();
    let lines = MemoryVistaFactory::new(store.clone())
        .from_yaml(LINE)
        .unwrap();
    let column = lines.column("total").unwrap();
    assert!(column.is_computed());
    assert_eq!(column.expression(), Some("row.qty * 2"));

    let rec: Record<CborValue> = [
        ("pnum".to_string(), CborValue::Integer(1.into())),
        ("qty".to_string(), CborValue::Integer(3.into())),
        ("total".to_string(), CborValue::Integer(99.into())),
    ]
    .into_iter()
    .collect();
    lines.insert_value("l1", &rec).await.unwrap();

    let stored = store.table("line").get("l1").unwrap();
    assert!(!stored.contains_key("total"), "{stored:?}");
    let read = lines.get_value("l1").await.unwrap().unwrap();
    assert_eq!(read.get("total"), Some(&CborValue::Integer(6.into())));
    assert_eq!(read.get("big"), Some(&CborValue::Bool(true)));
    assert_eq!(read.get("product_ref"), Some(&CborValue::Text("p1".into())));
}

#[tokio::test]
async fn an_expression_that_does_not_compile_fails_the_build() {
    let f = MemoryVistaFactory::new(MemoryStore::new());
    let Err(err) = f.from_yaml(&LINE.replace("row.qty * 2", "row.qty *")) else {
        panic!("a broken script must fail the build");
    };
    assert!(names_column(&err, "total"), "{err:?}");
}

fn names_column(err: &VantageError, column: &str) -> bool {
    err.context
        .get("column")
        .is_some_and(|v| v.trim_matches('"') == column)
}
