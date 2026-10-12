use super::*;
use crate::mocks::MockShell;
use crate::{Capability, Column, ColumnFlag, VistaMetadata};

fn rec(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string())))
        .collect()
}

fn target(metadata: VistaMetadata) -> Vista {
    Vista::new(
        "order",
        MockShell::new()
            .with_metadata(metadata)
            .with_capabilities([Capability::FilterOps].into_iter().collect()),
    )
}

fn orders() -> Reference {
    Reference::has_many("orders", "order", JoinKey::new("id", "client_id"))
}

fn order_schema() -> VistaMetadata {
    VistaMetadata::new()
        .with_column(Column::new("id", "string").with_flag(ColumnFlag::Id))
        .with_column(Column::new("client_id", "string"))
        .with_column(Column::new("region", "string"))
        .with_id_column("id")
}

#[test]
fn narrows_through_the_vista_with_the_parent_value() {
    let narrowed = orders()
        .narrow(target(order_schema()), &rec(&[("id", "alice")]))
        .unwrap();
    let condition = &narrowed.query().conditions[0];
    assert_eq!(condition.column, "client_id");
    assert_eq!(condition.value, CborValue::Text("alice".into()));
}

#[test]
fn every_key_narrows() {
    let reference = Reference::has_many("orders", "order", JoinKey::new("id", "client_id"))
        .with_key(JoinKey::new("region", "region"));
    let narrowed = reference
        .narrow(
            target(order_schema()),
            &rec(&[("id", "alice"), ("region", "eu")]),
        )
        .unwrap();
    let columns: Vec<_> = narrowed
        .query()
        .conditions
        .iter()
        .map(|c| c.column.as_str())
        .collect();
    assert_eq!(columns, ["client_id", "region"]);
}

#[test]
fn a_contained_relation_has_nothing_to_narrow_by() {
    let err = Reference::contains_many("lines", "lines")
        .narrow(target(order_schema()), &rec(&[("id", "alice")]))
        .unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");
    assert_eq!(
        err.context.get("relation").map(String::as_str),
        Some("\"lines\"")
    );
}

/// The fields are public, so a join can lose its keys after it is built;
/// narrowing it then is an error, not the bare target.
#[test]
fn a_join_whose_keys_were_emptied_has_nothing_to_narrow_by() {
    let mut reference = orders();
    reference.keys.clear();
    let err = reference
        .narrow(target(order_schema()), &rec(&[("id", "alice")]))
        .unwrap_err();
    assert!(err.is_incorrect_usage(), "{err:?}");
}

#[test]
fn a_missing_parent_column_names_relation_and_column() {
    let err = orders()
        .narrow(target(order_schema()), &rec(&[]))
        .unwrap_err();
    assert_eq!(
        err.context.get("relation").map(String::as_str),
        Some("\"orders\"")
    );
    assert_eq!(
        err.context.get("column").map(String::as_str),
        Some("\"id\"")
    );
}

#[cfg(feature = "rhai")]
#[test]
fn a_computed_join_column_stays_a_refusal() {
    let schema = VistaMetadata::new()
        .with_column(Column::new("id", "string"))
        .with_column(
            Column::new("client_id", "string")
                .with_expression("row.id")
                .unwrap(),
        )
        .with_id_column("id");
    let err = orders()
        .narrow(target(schema), &rec(&[("id", "alice")]))
        .unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
    assert_eq!(
        err.context.get("relation").map(String::as_str),
        Some("\"orders\"")
    );
    assert_eq!(
        err.context.get("column").map(String::as_str),
        Some("\"client_id\"")
    );
}

#[test]
fn a_target_without_an_id_column_names_the_relation() {
    let client = Reference::has_one("client", "client", JoinKey::to_target_id("client_id"));
    let err = client
        .narrow(target(VistaMetadata::new()), &rec(&[("client_id", "a")]))
        .unwrap_err();
    assert!(!err.is_unsupported() && !err.is_unimplemented(), "{err:?}");
    assert_eq!(
        err.context.get("relation").map(String::as_str),
        Some("\"client\"")
    );
    assert_eq!(
        err.context.get("column").map(String::as_str),
        Some("\"client_id\"")
    );
}
