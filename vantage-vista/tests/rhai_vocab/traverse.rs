//! `ref` traversal: `limit(n)` and `offset(n)` pick the parent rows followed, and an empty
//! parent set gives an empty target without an `in ()` condition.

use std::sync::Arc;

use ciborium::Value as CborValue;
use serde_json::json;
use vantage_memory::{MemoryCatalog, MemoryStore};
use vantage_rhai::Host;
use vantage_types::Record;
use vantage_vista::rhai::preview_script;
use vantage_vista::{Column, ColumnFlag, JoinKey, Reference, VistaMetadata, VistaResolver};

use super::support::{host_with, json, read_write, read_write_terminals, run, store};

fn text_row(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string())))
        .collect()
}

/// Clients c1 Ada, c2 Ben, c3 Cy; orders o1, o2 (c1), o3 (c2), o4 (c3).
fn shop() -> Arc<dyn VistaResolver> {
    let store = MemoryStore::new();
    for (id, name) in [("c1", "Ada"), ("c2", "Ben"), ("c3", "Cy")] {
        store
            .table("client")
            .insert_as(id, text_row(&[("name", name)]))
            .unwrap();
    }
    for (id, client) in [("o1", "c1"), ("o2", "c1"), ("o3", "c2"), ("o4", "c3")] {
        store
            .table("order")
            .insert_as(id, text_row(&[("client", client)]))
            .unwrap();
    }
    let catalog = MemoryCatalog::new(store);
    catalog.register(
        "client",
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
            .with_column(Column::new("name", "String").with_flag(ColumnFlag::Orderable))
            .with_id_column("id")
            .with_reference(Reference::has_many(
                "orders",
                "order",
                JoinKey::new("id", "client"),
            )),
    );
    catalog.register(
        "order",
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
            .with_column(Column::new("client", "String"))
            .with_id_column("id")
            .with_reference(Reference::has_one(
                "client",
                "client",
                JoinKey::to_target_id("client"),
            )),
    );
    Arc::new(catalog)
}

fn shop_host(resolver: Arc<dyn VistaResolver>) -> Host {
    host_with(resolver, read_write_terminals())
}

#[test]
fn ref_follows_only_the_parent_rows_in_its_window() {
    let host = shop_host(shop());
    // Sorted by name, descending: Cy (o4), Ben (o3), Ada (o1, o2).
    for (window, orders) in [
        ("limit(2)", json!(["o3", "o4"])),
        ("limit(1)", json!(["o4"])),
        ("offset(1).limit(1)", json!(["o3"])),
        ("offset(2)", json!(["o1", "o2"])),
    ] {
        let script =
            format!(r#"table("client").sort("name", "desc").{window}.ref("orders").ids()"#);
        assert_eq!(json(&host, &script), orders, "{script}");
    }
}

#[test]
fn ref_over_no_rows_is_empty() {
    let host = shop_host(shop());
    for script in [
        r#"table("client").where("name", "Nobody").ref("orders").list()"#,
        r#"table("order").where("client", "c9").ref("client").list()"#,
    ] {
        assert_eq!(json(&host, script), json!([]), "{script}");
    }
    assert_eq!(
        json(
            &host,
            r#"table("client").where("name", "Nobody").ref("orders").count()"#
        ),
        json!(0)
    );
}

#[test]
fn ref_over_no_rows_takes_no_writes() {
    let host = shop_host(shop());
    let nobody = r#"table("client").where("name", "Nobody").ref("orders")"#;
    let err = run(&host, &format!(r#"{nobody}.insert(#{{ id: "o9" }})"#)).unwrap_err();
    assert!(err.contains("holds no rows"), "{err}");
    assert_eq!(
        json(
            &host,
            &format!(r#"{nobody}.patch("o1", #{{ client: "c2" }})"#)
        ),
        json!(false)
    );
    assert_eq!(
        json(&host, &format!(r#"{nobody}.delete("o1")"#)),
        json!(true)
    );
    assert_eq!(
        json(&host, r#"table("order").ids()"#),
        json!(["o1", "o2", "o3", "o4"])
    );
}

#[test]
fn ref_over_no_rows_sends_no_condition() {
    let preview = preview_script(
        r#"table("client").where("name", "Nobody").ref("orders")"#.into(),
        shop(),
    )
    .unwrap();
    assert_eq!(preview["driver"], json!("memory"), "{preview}");
    assert!(preview["query"].is_null(), "{preview}");
}

#[test]
fn import_from_reads_only_the_source_rows_in_its_window() {
    let host = read_write(&store());
    assert_eq!(
        json(&host, r#"table("copy").import_from(table("t").limit(2))"#),
        json!({"inserted": 2, "skipped": 0, "rejected": 0, "cancelled": false})
    );
    assert_eq!(json(&host, r#"table("copy").count()"#), json!(2));
    json(&host, r#"table("tail").import_from(table("t").offset(1))"#);
    assert_eq!(json(&host, r#"table("tail").ids()"#), json!(["r2", "r3"]));
}
