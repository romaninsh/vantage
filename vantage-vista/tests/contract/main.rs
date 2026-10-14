//! `vantage_vista::contract` over the in-crate shells: `MockShell` and the
//! contained (embedded-in-row) shell.

use ciborium::Value as CborValue;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, ColumnFlag, Condition, JoinKey, Reference, TableShell, Vista,
    VistaCapabilities, VistaMetadata, contract, mocks::MockShell,
};

mod dotted;
mod window;

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
                .with_flag(ColumnFlag::Id)
                .with_flag(ColumnFlag::Orderable),
        )
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Searchable))
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
    let vista = Vista::new("product", product_shell());
    contract::check_advertised(&vista).await;
    contract::check_advertised_writes(&vista).await;
    assert!(vista.query().is_empty(), "the probes ran on clones");
}

/// The check calls the method, not just the flag: `MockShell` has no
/// `fetch_cursor`, so advertising it must fail.
#[tokio::test]
#[should_panic(expected = "`cursor` is advertised but `fetch_cursor` refuses it")]
async fn mock_contract_advertised_catches_a_missing_method() {
    let caps = VistaCapabilities::new().with(Capability::Cursor);
    let shell = product_shell().with_capabilities(caps);
    contract::check_advertised(&Vista::new("product", shell)).await;
}

#[tokio::test]
async fn mock_contract_clone() {
    let vista = Vista::new("product", product_shell());
    contract::check_clone(&vista, "category", t("a")).await;
}

/// Products joined to their category rows (`category` -> `category.id`).
fn joined_shell() -> MockShell {
    let categories = MockShell::new()
        .with_record("a", rec(&[("id", t("a"))]))
        .with_record("b", rec(&[("id", t("b"))]));
    let metadata = VistaMetadata::new()
        .with_column(
            Column::new("id", "String")
                .with_flag(ColumnFlag::Id)
                .with_flag(ColumnFlag::Orderable),
        )
        .with_column(Column::new("category", "String"))
        .with_id_column("id")
        .with_reference(Reference::has_one(
            "category",
            "category",
            JoinKey::new("category", "id"),
        ));
    product_shell()
        .with_metadata(metadata)
        .with_ref_target("category", categories)
}

#[tokio::test]
async fn mock_contract_traverse() {
    contract::check_advertised(&Vista::new("product", joined_shell())).await;
}

/// Returns join targets without narrowing them through the Vista.
#[derive(Clone)]
struct Unrecorded {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(Unrecorded, inner, {
    fn get_ref(
        &self,
        _relation: &str,
        _row: Option<&Record<CborValue>>,
    ) -> vantage_core::Result<Vista> {
        Ok(Vista::new("category", MockShell::new()))
    }
});

#[tokio::test]
#[should_panic(expected = "records no narrowing")]
async fn mock_contract_traverse_catches_an_unrecorded_join() {
    let vista = Vista::new("product", joined_shell()).map_shell(|inner| Unrecorded { inner });
    contract::check_advertised(&vista).await;
}

/// Takes every condition without narrowing by it, so a narrowed
/// `delete_all` empties the whole set while `ConfineWrites` is advertised.
#[derive(Clone)]
struct Unconfined {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(Unconfined, inner, {
    fn add_condition(&mut self, _condition: &Condition) -> vantage_core::Result<()> {
        Ok(())
    }
});

#[tokio::test]
#[should_panic(expected = "a delete_all narrowed to the probe row changed the un-narrowed count")]
async fn mock_contract_writes_catch_an_unconfined_delete_all() {
    let vista = Vista::new("product", product_shell()).map_shell(|inner| Unconfined { inner });
    assert!(vista.capabilities().has(Capability::ConfineWrites));
    contract::check_advertised_writes(&vista).await;
}

/// Accepts search, order and every condition without applying any of them.
#[derive(Clone)]
struct Ignoring {
    inner: Box<dyn TableShell>,
}

vantage_vista::forward_table_shell!(Ignoring, inner, {
    fn add_condition(&mut self, _condition: &Condition) -> vantage_core::Result<()> {
        Ok(())
    }
    fn set_search(&mut self, _text: Option<&str>) -> vantage_core::Result<()> {
        Ok(())
    }
    fn set_order(
        &mut self,
        _order: Option<(&str, vantage_vista::SortDirection)>,
    ) -> vantage_core::Result<()> {
        Ok(())
    }
});

fn ignoring() -> Vista {
    Vista::new("product", product_shell()).map_shell(|inner| Ignoring { inner })
}

#[tokio::test]
#[should_panic(expected = "counts 3 of 3 rows: the search is not applied")]
async fn mock_contract_narrowing_catches_an_unapplied_search() {
    contract::check_narrowing(&ignoring()).await;
}

#[tokio::test]
#[should_panic(expected = "both list row p1 first: the order is not applied")]
async fn mock_contract_narrowing_catches_an_unapplied_order() {
    contract::check_narrowing(&ignoring()).await;
}

#[tokio::test]
#[should_panic(expected = "still lists row p1: the condition is not applied")]
async fn mock_contract_narrowing_catches_an_unapplied_filter() {
    contract::check_narrowing(&ignoring()).await;
}

/// One order whose `lines` are an embedded array in the order row.
fn orders() -> Vista {
    let line_schema = VistaMetadata::new()
        .with_column(Column::new("product", "String"))
        .with_column(Column::new("quantity", "i64"));
    let shell = MockShell::new()
        .with_metadata(
            VistaMetadata::new()
                .with_column(
                    Column::new("id", "String")
                        .with_flag(ColumnFlag::Id)
                        .with_flag(ColumnFlag::Orderable),
                )
                .with_id_column("id"),
        )
        .with_contained(Reference::contains_many("lines", "lines"), line_schema);
    let line = |product: &str| cmap(&[("product", t(product)), ("quantity", 1.into())]);
    let order = rec(&[
        ("id", t("order1")),
        (
            "lines",
            CborValue::Array(vec![line("flux"), line("flux"), line("delorean")]),
        ),
    ]);
    Vista::new("order", shell.with_record("order1", order))
}

/// The `lines` of the order, as their own Vista.
async fn order_lines() -> Vista {
    let orders = orders();
    let order = orders.get_value("order1").await.unwrap().unwrap();
    orders.get_ref("lines", Some(&order)).unwrap()
}

/// Traversal probes the contained reference too.
#[tokio::test]
async fn contained_parent_contract_advertised() {
    contract::check_advertised(&orders()).await;
}

#[tokio::test]
async fn contained_contract_advertised() {
    let lines = order_lines().await;
    contract::check_advertised(&lines).await;
    contract::check_advertised_writes(&lines).await;
}

#[tokio::test]
async fn contained_contract_clone() {
    contract::check_clone(&order_lines().await, "product", t("flux")).await;
}
