use ciborium::Value as CborValue;
use vantage_dataset::ReadableValueSet;
use vantage_types::Record;

use super::{MockCalls, MockShell};
use crate::{Capability, Column, ColumnFlag, FilterOp, JoinKey, Reference, Vista, VistaMetadata};

mod conditions;
mod get_only;
mod refs;
mod writes;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

fn user_vista(source: MockShell) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Title))
        .with_column(Column::new("email", "String").with_flag(ColumnFlag::Hidden))
        .with_column(Column::new("vip_flag", "bool"))
        .with_id_column("id")
        .with_reference(Reference::has_many(
            "orders",
            "orders",
            JoinKey::new("id", "user_id"),
        ));
    Vista::new("users", source.with_metadata(metadata))
}

fn three_users() -> MockShell {
    let user =
        |name: &str, vip: bool| record(&[("name", text(name)), ("vip_flag", CborValue::Bool(vip))]);
    MockShell::new()
        .with_record("1", user("Alice", true))
        .with_record("2", user("Bob", false))
        .with_record("3", user("Carol", true))
}

#[test]
fn metadata_accessors_round_trip() {
    let vista = user_vista(MockShell::new());
    assert_eq!(vista.name(), "users");
    assert_eq!(vista.id_column(), Some("id"));
    assert_eq!(vista.title_columns(), vec!["name"]);
    let names: Vec<&str> = vista.columns().keys().map(String::as_str).collect();
    assert_eq!(names, ["id", "name", "email", "vip_flag"]);
    assert!(vista.column("email").unwrap().has_flag(ColumnFlag::Hidden));
    assert_eq!(
        vista.reference("orders").unwrap().foreign_key(),
        Some("user_id")
    );
    let caps = vista.capabilities();
    assert!(caps.has(Capability::Insert) && caps.has(Capability::Window));
    assert!(!caps.has(Capability::Subscribe));
}

#[tokio::test]
async fn reads_return_seeded_rows() {
    let vista = user_vista(three_users());
    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows["2"].get("name"), Some(&text("Bob")));
    let alice = vista.get_value("1").await.unwrap().unwrap();
    assert_eq!(alice.get("name"), Some(&text("Alice")));
    assert_eq!(vista.count().await.unwrap(), 3);
    let page = vista.fetch_window(1, 1).await.unwrap();
    assert_eq!(page.rows[0].0, "2");
    assert_eq!(page.total, Some(3));
}

#[tokio::test]
async fn calls_are_counted_across_clones() {
    let original = three_users();
    let vista = user_vista(original.clone());
    vista.count().await.unwrap();
    vista.fetch_window(0, 1).await.unwrap();
    assert_eq!(
        original.calls(),
        MockCalls {
            count: 1,
            fetch_window: 1,
            ..MockCalls::default()
        }
    );
}

#[tokio::test]
async fn conditions_filter_every_read_and_are_recorded() {
    let vista = user_vista(three_users())
        .with_condition("vip_flag", FilterOp::Eq, true)
        .unwrap();
    let rows = vista.list_values().await.unwrap();
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["1", "3"]);
    assert_eq!(vista.count().await.unwrap(), 2);
    assert!(vista.get_value("2").await.unwrap().is_none());
    assert!(vista.is_narrowed());
    assert_eq!(vista.query().conditions.len(), 1);

    let named = vista.with_condition("name", FilterOp::Gt, "B").unwrap();
    assert_eq!(named.list_values().await.unwrap().len(), 1);
}

#[tokio::test]
async fn order_and_search_apply_and_clear() {
    let refused = user_vista(three_users()).with_order("name", crate::SortDirection::Descending);
    assert!(
        refused.unwrap_err().is_unsupported(),
        "name isn't orderable"
    );

    let mut vista = user_vista(three_users());
    vista.set_search(Some("o")).unwrap();
    assert_eq!(vista.list_values().await.unwrap().len(), 2, "Bob, Carol");
    vista.set_search(None).unwrap();
    assert!(vista.query().search.is_none());
    assert_eq!(vista.list_values().await.unwrap().len(), 3);
}
