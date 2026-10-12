//! Which columns a [`MockShell`] condition may name.

use super::*;

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
/// column is rejected as a plain error rather than matching no row.
#[test]
fn a_dotted_path_is_rejected_not_matched_against_nothing() {
    let err = boxes()
        .with_condition("dims.w", FilterOp::Eq, 2)
        .unwrap_err();
    assert!(!err.is_unsupported() && !err.is_unimplemented(), "{err:?}");
    assert!(
        err.to_string().contains("nested paths are not evaluated"),
        "{err}"
    );
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
/// a dotted one is still rejected.
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
    assert!(vista.with_condition("colour.hue", FilterOp::Eq, 1).is_err());
}
