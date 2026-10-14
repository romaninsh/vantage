//! Conditions, search and order on the shell.

use bson::doc;
use vantage_vista::{
    Condition, FilterOp, NativeCondition, SortDirection, TableShell, VistaFactory,
};

use super::super::keyset;
use super::{CLIENT, client_vista, factory, shell_of};

#[tokio::test]
async fn search_and_order_replace_and_clear() {
    let mut vista = client_vista().await;
    vista.set_search(Some("a")).unwrap();
    vista.set_search(Some("b")).unwrap();
    assert!(vista.preview().query.unwrap().contains("<1 conditions>"));
    vista.set_search(None).unwrap();
    assert!(vista.preview().query.unwrap().contains("find({})"));

    let mut shell = shell_of(CLIENT).await;
    shell
        .set_order(Some(("name", SortDirection::Descending)))
        .unwrap();
    shell
        .set_order(Some(("_id", SortDirection::Ascending)))
        .unwrap();
    assert_eq!(
        keyset::effective_sort(&shell.table),
        vec![("_id".into(), 1)]
    );
    shell.set_order(None).unwrap();
    assert!(keyset::effective_sort(&shell.table).is_empty());
}

#[tokio::test]
async fn an_unknown_column_is_a_normal_error() {
    let mut shell = shell_of(CLIENT).await;
    let err = shell
        .add_condition(&Condition::new("nope", FilterOp::Gt, 1))
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
    let err = shell
        .set_order(Some(("nope", SortDirection::Ascending)))
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
    assert!(shell.table.conditions().next().is_none());
}

#[tokio::test]
async fn a_dotted_path_under_a_declared_column_filters_by_dot_notation() {
    let spec =
        "{ name: client, columns: { _id: { flags: [id] }, address: { mongo: { field: addr } } } }";
    let mut shell = shell_of(spec).await;
    shell
        .add_condition(&Condition::new("address.city", FilterOp::Eq, "Hill Valley"))
        .unwrap();
    let filter = format!("{:?}", shell.table.conditions().next().unwrap());
    assert!(filter.contains("addr.city"), "{filter}");

    let err = shell
        .add_condition(&Condition::new("nope.city", FilterOp::Eq, "x"))
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
    assert_eq!(shell.table.conditions().count(), 1);
}

#[tokio::test]
async fn a_shell_over_stored_documents_takes_equality_on_any_field() {
    let spec = serde_yaml_ng::from_str("{ name: bare, columns: {} }").unwrap();
    let bare = factory().await.build_from_spec(spec).unwrap();
    let narrowed = bare
        .clone()
        .with_condition("client_id", FilterOp::Eq, "c1")
        .unwrap();
    assert!(narrowed.is_narrowed());
    let err = bare
        .with_condition("client_id", FilterOp::Gt, "c1")
        .unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
}

#[tokio::test]
async fn native_conditions_are_recorded_by_key() {
    let vista = client_vista().await;
    let narrowed = vista
        .clone()
        .with_native_condition(NativeCondition::new(doc! { "n": 1 }, "n=1"))
        .unwrap();
    assert_eq!(narrowed.query().native, vec!["n=1".to_string()]);
    let err = vista
        .with_native_condition(NativeCondition::new(1_u8, "u8"))
        .unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");
}
