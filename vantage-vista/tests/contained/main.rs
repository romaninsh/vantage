//! Contained relations end-to-end at the Vista level (no real DB, no Diorama).
//!
//! Proves the eager writeback: mutating a contained sub-Vista immediately
//! patches the parent record's host column in the backing store.

use ciborium::Value as CborValue;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, ColumnFlag, Condition, Denials, FilterOp, Reference, ReferenceKind, Vista,
    VistaMetadata, VistaQuery, mocks::MockShell,
};

mod record_refs;

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

fn id_column() -> VistaMetadata {
    VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_id_column("id")
}

/// Order Vista with a `contains_many` `lines` relation, seeded with one order
/// (`order1`) whose `lines` column holds two embedded line objects.
fn order_vista() -> Vista {
    let line_schema = VistaMetadata::new()
        .with_column(Column::new("product", "String"))
        .with_column(Column::new("quantity", "i64"))
        .with_column(Column::new("price", "i64"));
    let order = rec(&[
        ("id", t("order1")),
        ("client", t("marty")),
        (
            "lines",
            CborValue::Array(vec![line("flux", 3, 120), line("delorean", 1, 135)]),
        ),
    ]);
    let shell = MockShell::new()
        .with_metadata(id_column().with_column(Column::new("client", "String")))
        .with_contained(Reference::contains_many("lines", "lines"), line_schema)
        .with_record("order1", order);
    Vista::new("order", shell)
}

#[tokio::test]
async fn references_report_the_contained_relation() {
    let vista = order_vista();
    let lines = vista.reference("lines").unwrap();
    assert_eq!(lines.kind, ReferenceKind::ContainsMany);
    assert_eq!(lines.host_column.as_deref(), Some("lines"));
}

#[tokio::test]
async fn inserting_a_line_eagerly_patches_the_parent_order() {
    let vista = order_vista();
    let order = vista.get_value("order1").await.unwrap().unwrap();
    let lines = vista.get_ref("lines", Some(&order)).unwrap();

    let new_id = lines
        .insert_return_id_value(&rec(&[
            ("product", t("hover")),
            ("quantity", i(2)),
            ("price", i(199)),
        ]))
        .await
        .unwrap();
    assert_eq!(new_id, "2"); // positional index

    assert_eq!(lines.list_values().await.unwrap().len(), 3);

    // The writeback landed: the parent's `lines` column is a 3-element array
    // ending with the new line.
    let reread = vista.get_value("order1").await.unwrap().unwrap();
    let CborValue::Array(stored) = reread.get("lines").unwrap() else {
        panic!("lines should be an array");
    };
    assert_eq!(stored.len(), 3);
    assert_eq!(stored[2], line("hover", 2, 199));
}

#[tokio::test]
async fn contains_one_patch_updates_the_embedded_object() {
    let shell = MockShell::new()
        .with_metadata(id_column().with_column(Column::new("name", "String")))
        .with_contained(
            Reference::contains_one("inventory", "inventory"),
            VistaMetadata::new().with_column(Column::new("stock", "i64")),
        )
        .with_record(
            "flux",
            rec(&[
                ("id", t("flux")),
                ("name", t("Flux Cupcake")),
                ("inventory", cmap(&[("stock", i(50))])),
            ]),
        );
    let vista = Vista::new("product", shell);

    let row = vista.get_value("flux").await.unwrap().unwrap();
    let inventory = vista.get_ref("inventory", Some(&row)).unwrap();

    // One embedded record, addressed by the fixed id "0".
    let seeded = inventory.list_values().await.unwrap();
    assert_eq!(seeded.len(), 1);
    assert_eq!(seeded["0"].get("stock"), Some(&i(50)));

    inventory
        .patch_value("0", &rec(&[("stock", i(100))]))
        .await
        .unwrap();

    let reread = vista.get_value("flux").await.unwrap().unwrap();
    assert_eq!(reread.get("inventory"), Some(&cmap(&[("stock", i(100))])));
}

#[tokio::test]
async fn contained_conditions_refuse_dotted_and_undeclared_columns() {
    let vista = order_vista();
    let order = vista.get_value("order1").await.unwrap().unwrap();
    let lines = vista.get_ref("lines", Some(&order)).unwrap();

    // A path into a declared column is refused, for the caller to evaluate.
    let dotted = lines
        .clone()
        .with_condition("product.name", FilterOp::Eq, "flux")
        .unwrap_err();
    assert!(dotted.is_unimplemented(), "{dotted:?}");
    let path = Condition::new("product.name", FilterOp::Eq, "flux");
    let (_, residual) = lines
        .clone()
        .with_query(&VistaQuery::new().with_condition(path.clone()))
        .unwrap();
    assert_eq!(residual.conditions, vec![path]);
    // An undeclared column, or a path under one, is a plain error.
    for column in ["sku", "sku.code"] {
        let undeclared = lines
            .clone()
            .with_condition(column, FilterOp::Eq, "flux")
            .unwrap_err();
        assert!(!undeclared.is_unimplemented(), "{undeclared:?}");
        assert!(
            undeclared.to_string().contains("Unknown column"),
            "{undeclared}"
        );
    }
}

/// Without a parent row a contained relation is its bare target: the
/// contained schema, no records, and no writes.
#[tokio::test]
async fn a_contained_relation_without_a_row_is_its_bare_target() {
    let bare = order_vista().get_ref("lines", None).unwrap();
    assert!(bare.column("quantity").is_some());
    assert!(bare.list_values().await.unwrap().is_empty());
    for write in [Capability::Insert, Capability::Update, Capability::Delete] {
        assert!(!bare.capabilities().has(write), "{write}");
    }
    let err = bare
        .insert_return_id_value(&Record::new())
        .await
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
}

/// Contained records write through the parent row, so a parent denied
/// `update`, or whose host column is read-only, denies its contained targets
/// every write, naming its rule.
#[tokio::test]
async fn contained_target_inherits_parent_update_denial() {
    let mut frozen = Denials::new();
    frozen.deny(Capability::Update, "policy/x.yaml#frozen");
    let mut lines_read_only = Denials::new();
    lines_read_only.deny_column("lines", "policy/x.yaml#lines");
    for (denials, rule) in [
        (frozen, "policy/x.yaml#frozen"),
        (lines_read_only, "policy/x.yaml#lines"),
    ] {
        let vista = order_vista().deny(denials);
        let order = vista.get_value("order1").await.unwrap().unwrap();
        let lines = vista.get_ref("lines", Some(&order)).unwrap();
        for write in [Capability::Insert, Capability::Update, Capability::Delete] {
            assert!(!lines.capabilities().has(write), "{rule}: {write}");
        }
        let line = rec(&[
            ("product", t("hover")),
            ("quantity", i(2)),
            ("price", i(199)),
        ]);
        let err = lines.insert_value("9", &line).await.unwrap_err();
        assert!(err.is_forbidden(), "{err:?}");
        assert_eq!(err.context.get("rule").map(String::as_str), Some(rule));
        assert!(lines.delete("0").await.unwrap_err().is_forbidden());

        let reread = vista.get_value("order1").await.unwrap().unwrap();
        assert_eq!(reread.get("lines"), order.get("lines"));
    }
}
