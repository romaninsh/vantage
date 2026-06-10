//! Contained relations end-to-end at the Vista level (no real DB, no Diorama).
//!
//! Proves the eager writeback: mutating a contained sub-Vista immediately
//! patches the parent record's host column in the backing store.

use ciborium::Value as CborValue;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_types::Record;
use vantage_vista::{
    Column, ContainedKind, ContainedSpec, Vista, VistaMetadata, mocks::mock_shell::MockShell,
};

fn t(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn i(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}
fn cmap(pairs: &[(&str, CborValue)]) -> CborValue {
    CborValue::Map(pairs.iter().map(|(k, v)| (t(k), v.clone())).collect())
}
fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).into(), v.clone()))
        .collect()
}

fn line(product: &str, qty: i64, price: i64) -> CborValue {
    cmap(&[
        ("product", t(product)),
        ("quantity", i(qty)),
        ("price", i(price)),
    ])
}

/// Order Vista with a `contains_many` `lines` relation, seeded with one order
/// (`order1`) whose `lines` column holds two embedded line objects.
fn order_vista() -> Vista {
    let line_cols = vec![
        Column::new("product", "String"),
        Column::new("quantity", "i64"),
        Column::new("price", "i64"),
    ];
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("client", "String"))
        .with_id_column("id")
        .with_contained(
            ContainedSpec::new("lines", "lines", ContainedKind::ContainsMany)
                .with_columns(line_cols),
        );
    let order = rec(&[
        ("id", t("order1")),
        ("client", t("marty")),
        (
            "lines",
            CborValue::Array(vec![line("flux", 3, 120), line("delorean", 1, 135)]),
        ),
    ]);
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("order1", order);
    Vista::new("order", Box::new(shell))
}

#[tokio::test]
async fn list_contained_reports_the_relation() {
    let vista = order_vista();
    assert_eq!(
        vista.list_contained(),
        vec![("lines".to_string(), ContainedKind::ContainsMany)]
    );
}

#[tokio::test]
async fn contains_many_traversal_reads_embedded_lines() {
    let vista = order_vista();
    let order = vista.get_value("order1").await.unwrap().unwrap();

    let lines = vista.get_ref("lines", &order).unwrap();
    let rows = lines.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows["0"].get("product"), Some(&t("flux")));
    assert_eq!(rows["1"].get("quantity"), Some(&i(1)));
}

#[tokio::test]
async fn inserting_a_line_eagerly_patches_the_parent_order() {
    let vista = order_vista();
    let order = vista.get_value("order1").await.unwrap().unwrap();
    let lines = vista.get_ref("lines", &order).unwrap();

    let new_id = lines
        .insert_return_id_value(&rec(&[
            ("product", t("hover")),
            ("quantity", i(2)),
            ("price", i(199)),
        ]))
        .await
        .unwrap();
    assert_eq!(new_id, "2"); // positional index

    // Sub-Vista now lists three.
    assert_eq!(lines.list_values().await.unwrap().len(), 3);

    // The writeback landed: re-read the parent order from the store and check
    // its `lines` column is a 3-element array ending with the new line.
    let reread = vista.get_value("order1").await.unwrap().unwrap();
    let CborValue::Array(stored) = reread.get("lines").unwrap() else {
        panic!("lines should be an array");
    };
    assert_eq!(stored.len(), 3);
    assert_eq!(stored[2], line("hover", 2, 199));
}

#[tokio::test]
async fn contains_one_patch_updates_the_embedded_object() {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id")
        .with_contained(
            ContainedSpec::new("inventory", "inventory", ContainedKind::ContainsOne)
                .with_columns(vec![Column::new("stock", "i64")]),
        );
    let product = rec(&[
        ("id", t("flux")),
        ("name", t("Flux Cupcake")),
        ("inventory", cmap(&[("stock", i(50))])),
    ]);
    let vista = Vista::new(
        "product",
        Box::new(
            MockShell::new()
                .with_metadata(metadata)
                .with_record("flux", product),
        ),
    );

    let row = vista.get_value("flux").await.unwrap().unwrap();
    let inventory = vista.get_ref("inventory", &row).unwrap();

    // One embedded record, addressed by the fixed id "0".
    let seeded = inventory.list_values().await.unwrap();
    assert_eq!(seeded.len(), 1);
    assert_eq!(seeded["0"].get("stock"), Some(&i(50)));

    inventory
        .patch_value("0", &rec(&[("stock", i(100))]))
        .await
        .unwrap();

    // Writeback patched the product's embedded inventory object.
    let reread = vista.get_value("flux").await.unwrap().unwrap();
    assert_eq!(reread.get("inventory"), Some(&cmap(&[("stock", i(100))])));
}
