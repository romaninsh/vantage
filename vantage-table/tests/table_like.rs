//! Vista smoke tests that previously exercised `AnyTable` against
//! `MockTableSource`. AnyTable is being retired across the workspace
//! (see plans for vantage-vista's stage 9 decommission); the replacement
//! is `vantage_vista::Vista` carrying a `TableShell`. These tests use
//! the in-memory [`MockShell`](vantage_vista::mocks::MockShell) so they
//! cover Vista plumbing without touching a real database.

use ciborium::Value as CborValue;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_types::Record;
use vantage_vista::{
    Column as VistaColumn, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell,
};

fn empty_vista(name: &str) -> Vista {
    let shell = MockShell::new();
    Vista::new(name.to_string(), Box::new(shell))
}

fn vista_with_columns(name: &str, columns: &[&str]) -> Vista {
    let mut metadata = VistaMetadata::new().with_id_column("id");
    metadata.columns.insert(
        "id".to_string(),
        VistaColumn::new("id".to_string(), "string"),
    );
    for col in columns {
        metadata
            .columns
            .insert(col.to_string(), VistaColumn::new(col.to_string(), "string"));
    }
    let shell = MockShell::new()
        .with_capabilities(VistaCapabilities {
            can_count: true,
            can_insert: true,
            can_update: true,
            can_delete: true,
            ..VistaCapabilities::default()
        })
        .with_metadata(metadata);
    Vista::new(name.to_string(), Box::new(shell))
}

#[tokio::test]
async fn vista_carries_table_name_and_columns() {
    let vista = vista_with_columns("users", &["name", "email"]);

    assert_eq!(vista.name(), "users");
    let cols = vista.get_column_names();
    assert!(cols.contains(&"id"));
    assert!(cols.contains(&"name"));
    assert!(cols.contains(&"email"));
    assert_eq!(vista.get_id_column(), Some("id"));
    assert_eq!(vista.driver(), "mock");
}

#[tokio::test]
async fn multiple_vistas_live_in_same_collection() {
    let users = vista_with_columns("users", &["name"]);
    let orders = vista_with_columns("orders", &["amount"]);

    let vistas: Vec<Vista> = vec![users, orders];
    for v in &vistas {
        assert!(!v.name().is_empty());
        assert_eq!(v.driver(), "mock");
    }
}

#[tokio::test]
async fn vista_value_round_trip() {
    let vista = vista_with_columns("items", &["name", "price"]);

    let mut record = Record::new();
    record.insert("name".to_string(), CborValue::Text("Test Item".into()));
    record.insert("price".to_string(), CborValue::Integer(99.into()));
    record.insert("available".to_string(), CborValue::Bool(true));

    vista
        .insert_value(&"item1".to_string(), &record)
        .await
        .expect("insert_value should succeed");

    let retrieved = vista
        .get_value(&"item1".to_string())
        .await
        .expect("get_value should not error")
        .expect("item1 should exist");
    assert_eq!(
        retrieved.get("name"),
        Some(&CborValue::Text("Test Item".into()))
    );
    assert_eq!(retrieved.get("available"), Some(&CborValue::Bool(true)));
}

#[tokio::test]
async fn vista_count_and_some_value() {
    let vista = vista_with_columns("items", &["name"]);

    let mut a = Record::new();
    a.insert("name".to_string(), CborValue::Text("Alpha".into()));
    vista
        .insert_value(&"a".to_string(), &a)
        .await
        .expect("insert a");

    let mut b = Record::new();
    b.insert("name".to_string(), CborValue::Text("Bravo".into()));
    vista
        .insert_value(&"b".to_string(), &b)
        .await
        .expect("insert b");

    assert_eq!(vista.get_count().await.expect("count"), 2);
    let some = vista
        .get_some_value()
        .await
        .expect("get_some_value")
        .expect("some row");
    assert!(["a".to_string(), "b".to_string()].contains(&some.0));
}

#[tokio::test]
async fn vista_insert_return_id_value() {
    let vista = vista_with_columns("items", &["name"]);

    let mut record = Record::new();
    record.insert("name".to_string(), CborValue::Text("auto-generated".into()));

    let id = vista
        .insert_return_id_value(&record)
        .await
        .expect("auto id insert");
    assert!(!id.is_empty());

    let fetched = vista
        .get_value(&id)
        .await
        .expect("get")
        .expect("just-inserted row");
    assert_eq!(
        fetched.get("name"),
        Some(&CborValue::Text("auto-generated".into()))
    );
}

#[test]
fn empty_vista_advertises_name_and_no_columns() {
    let vista = empty_vista("empty");
    assert_eq!(vista.name(), "empty");
    assert!(vista.get_column_names().is_empty());
    assert_eq!(vista.get_id_column(), None);
}
