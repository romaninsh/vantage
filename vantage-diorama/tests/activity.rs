//! Step 7: app-activity signal drives adaptive refresh cadence — fast while
//! active, slow on standby, paused while offline (resuming on reconnect).
//! Runs on a paused clock so ticks are deterministic.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Activity, ActivitySignal, Lens};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    let mut rec = Record::new();
    rec.insert("name".to_string(), CborValue::Text("alpha".into()));
    let shell = MockShell::new().with_metadata(metadata).with_record("a", rec);
    Vista::new("items", Box::new(shell))
}

/// Build a Lens that counts `on_refresh` invocations, with the given cadences
/// and a shared activity signal.
async fn counting_lens(
    cache_path: std::path::PathBuf,
    active: Duration,
    standby: Option<Duration>,
    signal: ActivitySignal,
    counter: Arc<AtomicU64>,
) -> Arc<Lens> {
    let mut b = Lens::new()
        .cache_at(cache_path)
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await
            }
        })
        .on_refresh(move |_dio| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .refresh_every(active)
        .activity_signal(signal);
    if let Some(s) = standby {
        b = b.standby_refresh_every(s);
    }
    Arc::new(b.build().expect("build lens"))
}

/// Advance the paused clock by `d` in small steps, yielding between so the
/// refresh loop's sleeps fire and its `on_refresh` futures run.
async fn drive(d: Duration) {
    let step = Duration::from_millis(50);
    let mut left = d;
    while !left.is_zero() {
        let s = step.min(left);
        tokio::time::advance(s).await;
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        left -= s;
    }
}

#[tokio::test(start_paused = true)]
async fn active_polls_fast_standby_polls_slow() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let signal = ActivitySignal::new(); // Active
    let count = Arc::new(AtomicU64::new(0));
    let lens = counting_lens(
        tmp.path().join("c.redb"),
        Duration::from_secs(1),
        Some(Duration::from_secs(10)),
        signal.clone(),
        count.clone(),
    )
    .await;
    let _dio = lens.make_dio(master()).await?;

    // Active: ~1 tick/sec.
    drive(Duration::from_secs(3)).await;
    let active_ticks = count.load(Ordering::SeqCst);
    assert!(
        active_ticks >= 2,
        "active should poll several times in 3s, got {active_ticks}"
    );

    // Standby: let any in-progress active sleep flush, then confirm the 10s
    // cadence suppresses ticks across a 5s window.
    signal.set(Activity::Standby);
    drive(Duration::from_secs(2)).await;
    let before = count.load(Ordering::SeqCst);
    drive(Duration::from_secs(5)).await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        before,
        "standby must not tick within its 10s interval"
    );

    // …but it does still poll, just slower — cross the interval.
    drive(Duration::from_secs(12)).await;
    assert!(
        count.load(Ordering::SeqCst) > before,
        "standby still polls after its (longer) interval"
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn offline_pauses_polling_and_resumes_on_reconnect() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let signal = ActivitySignal::new();
    let count = Arc::new(AtomicU64::new(0));
    let lens = counting_lens(
        tmp.path().join("c.redb"),
        Duration::from_secs(1),
        None,
        signal.clone(),
        count.clone(),
    )
    .await;
    let _dio = lens.make_dio(master()).await?;

    signal.set(Activity::Offline);
    drive(Duration::from_secs(5)).await;
    assert_eq!(
        count.load(Ordering::SeqCst),
        0,
        "offline must not poll at all"
    );

    signal.set(Activity::Active);
    drive(Duration::from_secs(3)).await;
    assert!(
        count.load(Ordering::SeqCst) >= 1,
        "polling resumes once back online"
    );
    Ok(())
}
