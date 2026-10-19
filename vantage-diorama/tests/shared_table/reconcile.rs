//! Changes that enter a Dio from outside its own writes, a reconcile and
//! `apply_change`, reach the other Dios over the table too (use case 7).

use tokio::sync::broadcast::error::TryRecvError;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{ChangeEvent, DioEvent};
use vantage_vista::{Condition, FilterOp, Vista};

use crate::fixtures::{
    Pair, cached_cell, cell, events_until, holds, ids, invoice, invoice_master, patch, until,
    view_showing,
};
use crate::support::{copy_master, memory_lens};

#[tokio::test]
async fn case7_a_reconcile_on_a_reaches_a_and_b() -> Result<()> {
    let Pair { a, b, shell } = Pair::eager().await;
    let a_view = view_showing(&a, &["x", "z"]).await;
    let b_view = view_showing(&b, &["x"]).await;

    shell.set_record("x", invoice("x", "7", "12"));
    a.reconcile(a.master().list_values().await?).await?;

    until("A and B both show 12", async || {
        cell(&a_view, "x", "amount").as_deref() == Some("12")
            && cell(&b_view, "x", "amount").as_deref() == Some("12")
    })
    .await;
    Ok(())
}

/// A new Dio's eager start reconciles into an empty cache. Every row reads
/// as news there, so none of it is published.
#[tokio::test]
async fn a_first_fill_publishes_nothing() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let mut b_bus = b.subscribe_events();

    let _c = a
        .derive()
        .with_condition(Condition::new("amount", FilterOp::Eq, "10"))
        .build()
        .await?;
    a.flash(patch("x", &[("amount", "11")])).await?;

    assert_eq!(events_until(&mut b_bus, "changed x").await, ["changed x"]);
    Ok(())
}

/// A notify-only change tells the others to re-read a row the publisher
/// still holds. A sibling sharing the publisher's cache table holds it too,
/// so it repaints the row rather than drop it.
#[tokio::test]
async fn a_notify_only_change_keeps_the_row_in_a_shared_store_sibling() -> Result<()> {
    let lens = memory_lens(copy_master());
    let shell = invoice_master();
    let a = lens
        .dio(Vista::new("invoice", shell.clone()))
        .build()
        .await?;
    let b = lens
        .dio(Vista::new("invoice", shell))
        .live()
        .build()
        .await?;
    let b_view = view_showing(&b, &["x", "z"]).await;
    let mut b_bus = b.subscribe_events();

    a.apply_change(ChangeEvent::Updated {
        id: "x".into(),
        new: None,
    })
    .await?;
    // The sentinel: a later write B hears.
    a.flash(patch("z", &[("amount", "6")])).await?;

    let seen = events_until(&mut b_bus, "changed z").await;
    assert!(!seen.contains(&"removed x".to_string()), "{seen:?}");
    assert_eq!(ids(&b_view), ["x", "z"]);
    Ok(())
}

/// B's set is narrowed, so a row gone from it only left B's set: A re-reads
/// it instead of deleting it.
#[tokio::test]
async fn a_narrowed_reconcile_drop_is_a_recheck_not_a_delete() -> Result<()> {
    let Pair { a, b, shell } = Pair::eager().await;
    shell.set_record("x", invoice("x", "8", "10"));

    b.refresh().await?;

    assert!(!holds(&b, "x").await, "B dropped x");
    until("A re-reads x as client 8", async || {
        cached_cell(&a, "x", "client_id").await.as_deref() == Some("8")
    })
    .await;
    Ok(())
}

#[tokio::test]
async fn apply_change_on_a_reaches_b() -> Result<()> {
    let Pair { a, b, .. } = Pair::eager().await;
    let b_view = view_showing(&b, &["x"]).await;

    a.apply_change(ChangeEvent::Updated {
        id: "x".into(),
        new: Some(invoice("x", "8", "10")),
    })
    .await?;

    until("B drops x", async || ids(&b_view).is_empty()).await;
    Ok(())
}

/// A reconcile that changed more rows than the hub carries comfortably
/// sends one "re-read your set" rather than a fact per row.
#[tokio::test]
async fn a_big_reconcile_diff_becomes_one_invalidation() -> Result<()> {
    const ROWS: usize = 200;
    let shell = invoice_master();
    for i in 0..ROWS {
        let id = format!("r{i:03}");
        shell.set_record(id.clone(), invoice(&id, "7", "1"));
    }
    let Pair { a, b, shell } = Pair::on(&memory_lens(copy_master()), shell).await;
    let mut b_bus = b.subscribe_events();

    for i in 0..ROWS {
        let id = format!("r{i:03}");
        shell.set_record(id.clone(), invoice(&id, "7", "2"));
    }
    a.refresh().await?;

    until("B re-read the changed rows", async || {
        cached_cell(&b, "r199", "amount").await.as_deref() == Some("2")
    })
    .await;
    let mut events = Vec::new();
    loop {
        match b_bus.try_recv() {
            Ok(event) => events.push(event),
            Err(TryRecvError::Lagged(n)) => panic!("B's bus overflowed by {n} events"),
            Err(_) => break,
        }
    }
    assert!(events.iter().any(|e| matches!(e, DioEvent::Refreshing)));
    let changed = events
        .iter()
        .filter(|e| matches!(e, DioEvent::RecordChanged { .. }))
        .count();
    assert!(changed < 10, "{changed} per-row events");
    Ok(())
}
