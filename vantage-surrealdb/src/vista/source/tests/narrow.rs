//! Conditions, native conditions, search and order.

use ciborium::Value as CborValue;
use vantage_vista::{FilterOp, NativeCondition, SortDirection};

use super::{clients, query};
use crate::types::AnySurrealType;

#[test]
fn search_and_order_replace_and_clear() {
    let mut vista = clients();
    vista.set_search(Some("doc")).unwrap();
    vista.set_search(Some("biff")).unwrap();
    let q = query(&vista);
    assert!(q.contains("biff") && !q.contains("doc"), "{q}");
    vista.set_search(None).unwrap();
    assert!(!query(&vista).contains("biff"), "{}", query(&vista));

    vista
        .set_order(Some(("name", SortDirection::Ascending)))
        .unwrap();
    assert!(query(&vista).contains("ORDER BY"), "{}", query(&vista));
    vista.set_order(None).unwrap();
    assert!(!query(&vista).contains("ORDER BY"), "{}", query(&vista));
}

#[test]
fn id_condition_coerces_a_string_into_a_record_id() {
    let vista = clients().with_id("client:marty").unwrap();
    assert!(
        query(&vista).contains("id = client:marty"),
        "{}",
        query(&vista)
    );
    let bare = clients().with_id("marty").unwrap();
    assert!(
        query(&bare).contains("id = client:marty"),
        "{}",
        query(&bare)
    );
}

#[test]
fn record_link_condition_takes_an_id_held_as_text() {
    let vista = clients()
        .with_condition("bakery", FilterOp::Eq, "bakery:hv")
        .unwrap()
        .with_condition("city", FilterOp::Eq, "a:b")
        .unwrap();
    let q = query(&vista);
    assert!(q.contains("bakery = bakery:hv"), "a record literal: {q}");
    assert!(q.contains("city = \"a:b\""), "text on a string column: {q}");

    let ids = CborValue::Array(vec!["marty".into(), "client:doc".into()]);
    let q = query(
        &clients()
            .with_condition("id", FilterOp::InSet, ids)
            .unwrap(),
    );
    assert!(q.contains("[client:marty, client:doc]"), "{q}");
}

#[test]
fn dotted_path_under_a_declared_column_is_walked_natively() {
    let vista = clients()
        .with_condition("bakery.name", FilterOp::Eq, "Hill Valley")
        .unwrap();
    assert!(
        query(&vista).contains("bakery.name = \"Hill Valley\""),
        "{}",
        query(&vista)
    );
    let err = clients()
        .with_condition("nope.name", FilterOp::Eq, "x")
        .map(drop)
        .expect_err("undeclared root");
    assert!(!err.is_unsupported(), "{err:?}");
}

#[test]
fn unknown_column_is_a_normal_error_and_records_nothing() {
    let mut vista = clients();
    let err = vista.add_condition("nope", FilterOp::Eq, "x").unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
    assert!(!vista.is_narrowed());
    let err = clients().with_order("nope", SortDirection::Ascending).err();
    assert!(err.is_some_and(|e| !e.is_unsupported()));
}

#[test]
fn not_in_set_is_refused_for_local_evaluation() {
    let mut vista = clients();
    let value = CborValue::Array(vec!["hv".into()]);
    let err = vista
        .add_condition("city", FilterOp::NotInSet, value)
        .unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
    assert!(vista.query().conditions.is_empty());
}

#[test]
fn native_condition_takes_an_expression_and_rejects_other_types() {
    let expr = crate::surreal_expr!("city != {}", (AnySurrealType::from(CborValue::from("tp"))));
    let key = expr.preview();
    let vista = clients()
        .with_native_condition(NativeCondition::new(expr, key.clone()))
        .unwrap();
    assert_eq!(vista.query().native, vec![key]);
    assert!(
        query(&vista).contains("city != \"tp\""),
        "{}",
        query(&vista)
    );

    let err = clients()
        .with_native_condition(NativeCondition::new(42_u8, "not-surreal"))
        .map(drop)
        .expect_err("foreign type");
    assert!(err.is_incorrect_usage(), "{err:?}");
}
