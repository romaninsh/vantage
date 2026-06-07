//! `Vista::index_key` produces a stable string identifying a query variant
//! (conditions + sort) so Diorama can cache one ordered index per variant and
//! reuse it when the same conditions/sort recur.

use ciborium::Value as CborValue;
use vantage_vista::{Column, SortDirection, Vista, VistaMetadata, mocks::mock_shell::MockShell};

fn t(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn i(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn runs_vista() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("branch", "String"))
        .with_column(Column::new("status", "String"))
        .with_id_column("id");
    let shell = MockShell::new().with_metadata(metadata);
    Vista::new("gh-workflow-runs", Box::new(shell))
}

fn other_vista() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    let shell = MockShell::new().with_metadata(metadata);
    Vista::new("gh-workflows", Box::new(shell))
}

#[test]
fn same_conditions_and_sort_yield_same_key() {
    let v = runs_vista();
    let conds = vec![("branch".to_string(), t("main"))];
    let sort = Some(("status", SortDirection::Ascending));
    assert_eq!(
        v.index_key(&conds, sort),
        v.index_key(&conds, sort),
        "identical query must produce identical key"
    );
}

#[test]
fn different_conditions_yield_different_keys() {
    let v = runs_vista();
    let a = v.index_key(&[("branch".to_string(), t("main"))], None);
    let b = v.index_key(&[("branch".to_string(), t("dev"))], None);
    assert_ne!(a, b, "different condition value must change the key");

    let none = v.index_key(&[], None);
    assert_ne!(a, none, "adding a condition must change the key");
}

#[test]
fn different_sort_yields_different_key() {
    let v = runs_vista();
    let asc = v.index_key(&[], Some(("status", SortDirection::Ascending)));
    let desc = v.index_key(&[], Some(("status", SortDirection::Descending)));
    let unsorted = v.index_key(&[], None);
    assert_ne!(asc, desc, "sort direction must change the key");
    assert_ne!(asc, unsorted, "presence of a sort must change the key");
}

#[test]
fn condition_order_does_not_change_key() {
    let v = runs_vista();
    let ab = v.index_key(
        &[
            ("branch".to_string(), t("main")),
            ("status".to_string(), i(1)),
        ],
        None,
    );
    let ba = v.index_key(
        &[
            ("status".to_string(), i(1)),
            ("branch".to_string(), t("main")),
        ],
        None,
    );
    assert_eq!(ab, ba, "condition order must be normalized away");
}

#[test]
fn different_vista_name_yields_different_key() {
    let runs = runs_vista();
    let workflows = other_vista();
    assert_ne!(
        runs.index_key(&[], None),
        workflows.index_key(&[], None),
        "the same empty query on two entities must not collide"
    );
}
