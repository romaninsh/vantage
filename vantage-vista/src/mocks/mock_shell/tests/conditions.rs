//! Which columns a [`MockShell`] condition may name.

use super::*;
use crate::{Condition, VistaQuery};

fn dims() -> CborValue {
    CborValue::Map(vec![(text("w"), CborValue::Integer(2.into()))])
}

fn boxes() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("dims", "map"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("b1", record(&[("id", text("b1")), ("dims", dims())]));
    Vista::new("box", shell)
}

/// The mock doesn't evaluate nested fields, so a dotted path into a declared
/// column is refused as `Unimplemented` rather than matching no row.
#[test]
fn a_dotted_path_is_refused_not_matched_against_nothing() {
    let err = boxes()
        .with_condition("dims.w", FilterOp::Eq, 2)
        .unwrap_err();
    assert!(err.is_unimplemented(), "{err:?}");
    assert!(err.to_string().contains("dotted path"), "{err}");
}

/// Through `with_query` the refused path comes back as residual, while the
/// conditions the mock takes narrow the vista.
#[tokio::test]
async fn with_query_hands_a_refused_dotted_path_back_as_residual() {
    let path = Condition::new("dims.w", FilterOp::Eq, 2);
    let query = VistaQuery::new()
        .with_condition(Condition::new("id", FilterOp::Eq, "b1"))
        .with_condition(path.clone());
    let (narrowed, residual) = boxes().with_query(&query).unwrap();
    assert_eq!(residual.conditions, vec![path]);
    assert_eq!(narrowed.query().conditions.len(), 1);
    assert_eq!(narrowed.list_values().await.unwrap().len(), 1);
}

/// A dotted path whose root isn't declared names an unknown column: a
/// plain error, never a refusal a caller would evaluate locally.
#[test]
fn a_dotted_path_under_an_unknown_root_is_a_plain_error() {
    let err = boxes()
        .with_condition("colour.hue", FilterOp::Eq, 1)
        .unwrap_err();
    assert!(!err.is_unsupported() && !err.is_unimplemented(), "{err:?}");
}

#[test]
fn an_undeclared_column_is_rejected() {
    let err = boxes()
        .with_condition("colour", FilterOp::Eq, "red")
        .unwrap_err();
    assert!(!err.is_unsupported() && !err.is_unimplemented(), "{err:?}");
    assert!(err.to_string().contains("Unknown column"), "{err}");
}

/// A mock that declares no columns is schemaless: any plain column narrows,
/// a dotted one is still refused.
#[tokio::test]
async fn a_schemaless_mock_takes_any_plain_column() {
    let shell = MockShell::new()
        .with_record("1", record(&[("colour", text("red"))]))
        .with_record("2", record(&[("colour", text("blue"))]));
    let vista = Vista::new("t", shell);
    let red = vista
        .clone()
        .with_condition("colour", FilterOp::Eq, "red")
        .unwrap();
    assert_eq!(red.list_values().await.unwrap().len(), 1);
    let err = vista
        .with_condition("colour.hue", FilterOp::Eq, 1)
        .unwrap_err();
    assert!(err.is_unimplemented(), "{err:?}");
}
