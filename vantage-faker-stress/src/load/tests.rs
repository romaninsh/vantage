use std::time::Duration;

use super::*;
use crate::sampler::Sampler;
use vantage_faker::{FakerColumn, FakerTable, SimStats, StaticEffect};

fn table() -> FakerTable {
    let mut id = FakerColumn::new("id", "string");
    id.flags.push("id".into());
    FakerTable::build(
        "t",
        vec![id, FakerColumn::new("note", "string")],
        "id",
        Box::new(StaticEffect { count: 5 }),
    )
}

async fn push_three(handle: &vantage_faker::FakerHandle) {
    for _ in 0..3 {
        handle.ctx().push();
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn counting_subscriber_sees_every_event() {
    let (vista, handle) = table().split();
    let load = attach(vista, &handle, None).await.unwrap();
    push_three(&handle).await;
    let t = load.totals();
    assert_eq!(t.delivered, 3);
    assert_eq!(t.backlog, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn dio_subscriber_applies_events() {
    let dir = tempfile::tempdir().unwrap();
    let lens = lens(dir.path()).unwrap();
    let (vista, handle) = table().split();
    let load = attach(vista, &handle, Some(&lens)).await.unwrap();
    push_three(&handle).await;
    assert_eq!(load.totals().delivered, 3);
    assert_eq!(load.scenery_rows(), Some(8));
}

/// A burst after an idle second, drained at once, must not read as the
/// burst size divided by the idle second's delivery rate. The single-thread
/// runtime keeps the subscriber from running until the test awaits, so the
/// burst tick always sees the full backlog.
#[tokio::test]
async fn burst_after_idle_second_reads_short_lag() {
    let (vista, handle) = table().split();
    let loads = vec![attach(vista, &handle, None).await.unwrap()];
    let mut sampler = Sampler::new();
    sampler.sample(SimStats::default(), sum(&loads));
    tokio::time::sleep(Duration::from_secs(1)).await;
    for _ in 0..63 {
        handle.ctx().push();
    }
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
    let (vista, handle) = table().split();
    let loads = vec![attach(vista, &handle, None).await.unwrap()];
    let mut sampler = Sampler::new();
    for _ in 0..10 {
        handle.ctx().push();
    }
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
