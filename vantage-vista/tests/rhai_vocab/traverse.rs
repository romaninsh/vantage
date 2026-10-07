//! `ref` traversal: `limit(n)` caps the parent rows followed, and an empty
//! parent set gives an empty target without an `in ()` condition.

use std::sync::Arc;

use ciborium::Value as CborValue;
use serde_json::json;
use vantage_memory::MemoryStore;
use vantage_memory::MemoryTableShell;
use vantage_memory::vista::Catalog;
use vantage_rhai::{Host, Limits};
use vantage_types::Record;
use vantage_vista::{
    Column, DataVocab, Reference, ReferenceKind, TargetResolver, Terminals, Vista, VistaMetadata,
    Writes, preview_script,
};

use super::support::{json, read_write, run, store};

fn text_row(pairs: &[(&str, &str)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), CborValue::Text(v.to_string())))
        .collect()
}

/// Clients c1 Ada, c2 Ben, c3 Cy; orders o1, o2 (c1), o3 (c2), o4 (c3).
fn shop() -> TargetResolver {
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
    let catalog = Catalog::new(store);
    catalog.register(
        "client",
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("name", "String").with_flag("orderable"))
            .with_id_column("id")
            .with_reference(Reference::new(
                "orders",
                "order",
                ReferenceKind::HasMany,
                "client",
            )),
    );
    catalog.register(
        "order",
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("client", "String"))
            .with_id_column("id")
            .with_reference(Reference::new(
                "client",
                "client",
                ReferenceKind::HasOne,
                "client",
            )),
    );
    Arc::new(move |name: &str| {
        let metadata = catalog.get(name).expect("shop table");
        let table = catalog.store().table(name);
        let shell = MemoryTableShell::new(table, metadata, catalog.clone());
        Ok(Vista::new(name, Box::new(shell)))
    })
}

fn shop_host(resolver: TargetResolver) -> Host {
    Host::builder(Limits::background())
        .vocab(DataVocab {
            resolver: Some(resolver),
            terminals: Terminals::ReadWrite {
                limit: None,
                writes: Writes::Allowed,
            },
        })
        .build()
}

#[test]
fn ref_follows_only_the_limited_parent_rows() {
    let host = shop_host(shop());
    assert_eq!(
        json(
            &host,
            r#"table("client").sort("name", "desc").limit(2).ref("orders").ids()"#
        ),
        json!(["o3", "o4"])
    );
    assert_eq!(
        json(
            &host,
            r#"table("client").sort("name", "desc").limit(1).ref("orders").ids()"#
        ),
        json!(["o4"])
    );
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
fn import_from_reads_only_the_limited_source_rows() {
    let host = read_write(&store());
    assert_eq!(
        json(&host, r#"table("copy").import_from(table("t").limit(2))"#),
        json!({"inserted": 2, "skipped": 0, "rejected": 0, "cancelled": false})
    );
    assert_eq!(json(&host, r#"table("copy").count()"#), json!(2));
}
