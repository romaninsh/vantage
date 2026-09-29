use std::time::Duration;

use super::*;
use crate::sampler::Sampler;
use ciborium::Value as CborValue;
use vantage_faker::SimStats;
use vantage_memory::{MemoryStore, MemoryTableHandle};
use vantage_types::Record;
use vantage_vista::{Column, flags};

fn metadata() -> VistaMetadata {
    VistaMetadata::new()
        .with_id_column("id")
        .with_column(
            Column::new("id", "string")
                .with_flag(flags::ID)
                .with_flag(flags::ORDERABLE),
        )
        .with_column(Column::new("note", "string"))
}

/// A store with table `t` holding five rows.
fn store() -> (MemoryStore, MemoryTableHandle) {
    let store = MemoryStore::new();
    let table = store.table("t");
    for _ in 0..5 {
        table.insert(note()).unwrap();
    }
    (store, table)
}

fn note() -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("note".to_string(), CborValue::Text("x".into()));
    r
}

async fn attach_to(store: &MemoryStore, table: &MemoryTableHandle) -> TableLoad {
    attach(table.clone(), metadata(), store, None)
        .await
        .unwrap()
}

fn push(table: &MemoryTableHandle, n: usize) {
    for _ in 0..n {
        table.insert(note()).unwrap();
    }
}

async fn push_three(table: &MemoryTableHandle) {
    push(table, 3);
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn counting_subscriber_sees_every_event() {
    let (store, table) = store();
    let load = attach_to(&store, &table).await;
    push_three(&table).await;
    let t = load.totals();
    assert_eq!(t.delivered, 3);
    assert_eq!(t.backlog, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_counts_as_delivered() {
    let (store, table) = store();
    let load = attach_to(&store, &table).await;
    table.set_quiet(true);
    push(&table, 2);
    table.set_quiet(false);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(load.totals().delivered, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn dio_watches_the_table() {
    let dir = tempfile::tempdir().unwrap();
    let lens = lens(dir.path()).unwrap();
    let (store, table) = store();
    let load = attach(table.clone(), metadata(), &store, Some(&lens))
        .await
        .unwrap();
    push_three(&table).await;
    assert_eq!(load.totals().delivered, 3);
    assert_eq!(load.scenery_rows(), Some(8));
}

/// A burst after an idle second, drained at once, must not read as the
/// burst size divided by the idle second's delivery rate. The single-thread
/// runtime keeps the subscriber from running until the test awaits, so the
/// burst tick always sees the full backlog.
#[tokio::test]
async fn burst_after_idle_second_reads_short_lag() {
    let (store, table) = store();
    let loads = vec![attach_to(&store, &table).await];
    let mut sampler = Sampler::new();
    sampler.sample(SimStats::default(), sum(&loads));
    tokio::time::sleep(Duration::from_secs(1)).await;
    push(&table, 63);
    let burst = sampler.sample(SimStats::default(), sum(&loads));
    tokio::time::sleep(Duration::from_secs(1)).await;
    let next = sampler.sample(SimStats::default(), sum(&loads));
    assert!(burst.lag_ms < 500.0, "burst tick: {}", burst.lag_ms);
    assert!(next.lag_ms < 500.0, "next tick: {}", next.lag_ms);
}

/// Blocking the single-thread runtime keeps the subscriber from draining,
/// so its lag keeps growing from one tick to the next.
#[tokio::test]
async fn blocked_subscriber_shows_growing_lag() {
    let (store, table) = store();
    let loads = vec![attach_to(&store, &table).await];
    let mut sampler = Sampler::new();
    push(&table, 10);
    sampler.sample(SimStats::default(), sum(&loads));
    std::thread::sleep(Duration::from_millis(300));
    let first = sampler.sample(SimStats::default(), sum(&loads));
    std::thread::sleep(Duration::from_millis(300));
    let second = sampler.sample(SimStats::default(), sum(&loads));
    assert!(first.lag_ms >= 300.0, "{}", first.lag_ms);
    assert!(
        second.lag_ms >= first.lag_ms + 300.0,
        "{first:?} {second:?}"
    );
}
