//! Staged values: a cancelled caller puts its staged value back, and a
//! failed flash under a stacked one leaves the later value standing.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, CacheTable, DioEvent, FlashOutcome, Lens};
use vantage_vista::Vista;

use super::{items, name_of, rename, seeded, staged};
use crate::support::spy_cache::{SpyCache, SpyTable};

/// A caller cancelled while its value is being staged: the flash never
/// reaches the queue, the staged value is put back, and nothing is written.
#[tokio::test]
async fn a_caller_cancelled_mid_stage_leaves_no_staged_value() -> Result<()> {
    let table = Arc::new(SpyTable::default());
    let lens = Lens::builder()
        .cache(Cache::backend(Arc::new(SpyCache(table.clone()))))
        .build()?;
    let shell = items();
    let dio = seeded(&lens, Vista::new("items", shell.clone())).await?;

    table.stall_next_insert.store(true, Ordering::SeqCst);
    let caller = tokio::spawn({
        let dio = dio.clone();
        async move { dio.flash(rename("edited")).await }
    });
    table.stalled.notified().await;
    assert_eq!(
        name_of(table.inner.get_value("a").await?).as_deref(),
        Some("edited")
    );

    let mut bus = dio.subscribe_events();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let restored = async {
        while !matches!(bus.recv().await, Ok(DioEvent::RecordChanged { id }) if id == "a") {}
    };
    tokio::time::timeout(Duration::from_secs(5), restored)
        .await
        .expect("the staged row was never restored");

    let row = table.inner.get_value("a").await?;
    assert_eq!(name_of(row).as_deref(), Some("one"), "pre-image restored");
    let upstream = shell.get_record("a");
    assert_eq!(
        name_of(upstream).as_deref(),
        Some("one"),
        "nothing was written"
    );
    Ok(())
}

/// The gate each held flash waits on, keyed by the id it writes.
type Gates = Arc<Mutex<Vec<(String, Arc<Semaphore>)>>>;

/// A writer that holds each flash on its own gate, then fails the one
/// renaming to `fail` and confirms the rest.
fn gated_writer(gates: Gates) -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::memory())
        .writer(move |_cx, flash| {
            let name = match flash.patch().get("name") {
                Some(CborValue::Text(name)) => name.clone(),
                _ => String::new(),
            };
            let gate = gates
                .lock()
                .unwrap()
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, g)| g.clone());
            async move {
                if let Some(gate) = gate {
                    gate.acquire().await.expect("gate open").forget();
                }
                if name == "fail" {
                    return Err(vantage_core::error!("route rejected the flash"));
                }
                Ok(FlashOutcome::applied())
            }
        })
        .build()
        .expect("lens")
}

/// Two flashes stacked on one row. The first fails while the second is
/// staged over it: its rollback leaves the second's staged value alone, and
/// the second then settles.
#[tokio::test]
async fn a_failed_flash_under_a_stacked_one_keeps_the_later_value() -> Result<()> {
    let (first_gate, second_gate) = (Arc::new(Semaphore::new(0)), Arc::new(Semaphore::new(0)));
    let gates = vec![
        ("fail".to_string(), first_gate.clone()),
        ("kept".to_string(), second_gate.clone()),
    ];
    let lens = gated_writer(Arc::new(Mutex::new(gates)));
    let dio = seeded(&lens, Vista::new("items", items())).await?;
    let mut bus = dio.subscribe_events();

    let first = tokio::spawn({
        let dio = dio.clone();
        async move { dio.flash(rename("fail")).await }
    });
    staged(&mut bus).await;
    let second = tokio::spawn({
        let dio = dio.clone();
        async move { dio.flash(rename("kept")).await }
    });
    staged(&mut bus).await;

    first_gate.add_permits(1);
    assert!(first.await.unwrap().is_err());
    let row = dio.cached().get_value("a").await?;
    assert_eq!(
        name_of(row).as_deref(),
        Some("kept"),
        "the stacked value stands"
    );

    second_gate.add_permits(1);
    second.await.unwrap()?;
    let row = dio.cached().get_value("a").await?;
    assert_eq!(name_of(row).as_deref(), Some("kept"));
    Ok(())
}
