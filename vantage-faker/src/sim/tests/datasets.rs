//! Sims over tables a `DatasetGen` seeded: relational and fan-out tables,
//! and tables keyed by an id column other than `id`.

use super::*;
use crate::{DatasetGen, FakerColumn, FanOut, TableGen};

/// A `client` table and an `invoice` table referencing it through
/// `client_id`, with `fan_out` if given.
fn clients_and_invoices(fan_out: Option<FanOut>) -> MemoryStore {
    let store = MemoryStore::new();
    let mut invoice = TableGen::new("invoice")
        .column(FakerColumn::new("client_id", "string"))
        .reference("client_id", "client")
        .count(4);
    if let Some(fan_out) = fan_out {
        invoice = invoice.fan_out(fan_out);
    }
    DatasetGen::new(Some(5))
        .table(TableGen::new("client").count(3))
        .table(invoice)
        .generate(&store)
        .unwrap();
    store
}

fn pay_every_invoice(store: &MemoryStore) {
    let script = r#"for id in ids() { patch(id, #{ paid: true }); }"#;
    let engine = SimEngine::builder()
        .store(store)
        .sim(SimDef::new("payer", "invoice", script))
        .manual_clock(start())
        .start()
        .unwrap();
    run_for(&engine, 1, 1);
    let inv = store.table("invoice");
    assert!(!inv.ids().is_empty());
    assert!(
        inv.ids()
            .iter()
            .all(|id| inv.get(id).unwrap().get("paid") == Some(&CborValue::Bool(true)))
    );
}

#[test]
fn sims_write_a_relational_table() {
    pay_every_invoice(&clients_and_invoices(None));
}

#[test]
fn sims_write_a_fan_out_table() {
    let store = clients_and_invoices(Some(FanOut {
        column: "client_id".into(),
        min: 2,
        max: 3,
    }));
    assert!((6..=9).contains(&store.table("invoice").len()));
    pay_every_invoice(&store);
}

#[test]
fn sims_insert_and_find_by_a_custom_id_column() {
    let store = MemoryStore::new();
    DatasetGen::new(Some(2))
        .table(
            TableGen::new("product")
                .id_column("code")
                .column(FakerColumn::new("name", "string"))
                .count(3),
        )
        .generate(&store)
        .unwrap();
    let script = r#"
        let id = insert(#{ code: "x", name: "new" });
        let hits = find(#{ code: "x" });
        insert(#{ code: "result", name: id + ":" + hits.len() + ":" + hits[0] });
    "#;
    let engine = SimEngine::builder()
        .store(&store)
        .sim(SimDef::new("add", "product", script))
        .manual_clock(start())
        .start()
        .unwrap();
    run_for(&engine, 1, 1);
    let product = store.table("product");
    assert_eq!(product.len(), 5);
    assert_eq!(text(&product.get("x").unwrap(), "name"), "new");
    assert_eq!(text(&product.get("result").unwrap(), "name"), "x:1:x");
}
