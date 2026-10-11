//! `vantage_vista::contract` over the in-crate shells: `MockShell` and the
//! contained (embedded-in-row) shell.

use ciborium::Value as CborValue;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::{
    Column, ContainedKind, ContainedSpec, Vista, VistaCapabilities, VistaMetadata, contract,
    mocks::mock_shell::MockShell,
};

fn t(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).into(), v.clone()))
        .collect()
}

fn cmap(pairs: &[(&str, CborValue)]) -> CborValue {
    CborValue::Map(pairs.iter().map(|(k, v)| (t(k), v.clone())).collect())
}

/// Three products, two of them in category `a`.
fn product_shell() -> MockShell {
    let metadata = VistaMetadata::new()
        .with_column(
            Column::new("id", "String")
                .with_flag("id")
                .with_flag("orderable"),
        )
        .with_column(Column::new("name", "String").with_flag("searchable"))
        .with_column(Column::new("category", "String"))
        .with_id_column("id");
    MockShell::new()
        .with_metadata(metadata)
        .with_record(
            "p1",
            rec(&[("id", t("p1")), ("name", t("apple")), ("category", t("a"))]),
        )
        .with_record(
            "p2",
            rec(&[("id", t("p2")), ("name", t("pear")), ("category", t("a"))]),
        )
        .with_record(
            "p3",
            rec(&[("id", t("p3")), ("name", t("kiwi")), ("category", t("b"))]),
        )
}

#[tokio::test]
async fn mock_contract_advertised() {
    let vista = Vista::new("product", Box::new(product_shell()));
    contract::check_advertised(&vista).await;
    contract::check_advertised_writes(&vista).await;
}

/// The check calls the method, not just the flag: `MockShell` has no
/// `fetch_next`, so advertising it must fail.
#[tokio::test]
#[should_panic(expected = "`can_fetch_next` is advertised but `fetch_next` refuses it")]
async fn mock_contract_advertised_catches_a_missing_method() {
    let shell = product_shell().with_capabilities(VistaCapabilities {
        can_fetch_next: true,
        ..VistaCapabilities::default()
    });
    contract::check_advertised(&Vista::new("product", Box::new(shell))).await;
}

#[tokio::test]
async fn mock_contract_clone() {
    let vista = Vista::new("product", Box::new(product_shell()));
    contract::check_clone(&vista, "category", t("a")).await;
}

/// The `lines` of one order, held as an embedded array in the order row.
async fn order_lines() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id")
        .with_contained(
            ContainedSpec::new("lines", "lines", ContainedKind::ContainsMany).with_columns(vec![
                Column::new("product", "String"),
                Column::new("quantity", "i64"),
            ]),
        );
    let line = |product: &str| cmap(&[("product", t(product)), ("quantity", 1.into())]);
    let order = rec(&[
        ("id", t("order1")),
        (
            "lines",
            CborValue::Array(vec![line("flux"), line("flux"), line("delorean")]),
        ),
    ]);
    let orders = Vista::new(
        "order",
        Box::new(
            MockShell::new()
                .with_metadata(metadata)
                .with_record("order1", order),
        ),
    );
    let order = orders.get_value("order1").await.unwrap().unwrap();
    orders.get_ref("lines", &order).unwrap()
}

#[tokio::test]
async fn contained_contract_advertised() {
    contract::check_advertised(&order_lines().await).await;
}

#[tokio::test]
#[ignore = "v2 phase 1: shell not cloneable"]
async fn contained_contract_clone() {
    contract::check_clone(&order_lines().await, "product", t("flux")).await;
}
