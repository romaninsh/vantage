//! `vantage_vista::contract` over `AggregateShell`: the master Vista of a
//! derived Dio, grouping a mock source by `status`.

mod support;

use vantage_diorama_aggregate::{AggregateLens, DerivedDio, GroupBy, Reduce};
use vantage_types::Record;
use vantage_vista::{Column, contract};

use support::{DEBOUNCE, int, master, record, source, text};

/// Groups `open` (two rows), `done` and `held` (one row each), so the
/// derived set has three rows and `orders == 1` keeps two of them.
async fn derived() -> DerivedDio {
    let shell = master(&[("status", "String")]);
    for (id, status) in [("a", "open"), ("b", "open"), ("c", "done"), ("d", "held")] {
        shell.set_record(id, record(&[("status", text(status))]));
    }
    let orders = Reduce::new(vec![Column::new("orders", "i64")], |_key, rows| {
        let mut out = Record::new();
        out.insert("orders".to_string(), int(rows.len() as i64));
        out
    });
    let src = source(shell).await;
    AggregateLens::in_memory()
        .debounce(DEBOUNCE)
        .build()
        .expect("aggregate lens builds")
        .derive(&src, "by_status", GroupBy::column("status", orders))
        .await
        .expect("derived dio")
}

#[tokio::test]
async fn aggregate_contract_advertised() {
    contract::check_advertised(derived().await.master()).await;
}

#[tokio::test]
async fn aggregate_contract_clone() {
    contract::check_clone(derived().await.master(), "orders", int(1)).await;
}
