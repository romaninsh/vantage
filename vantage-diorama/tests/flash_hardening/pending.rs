//! Reconcile-while-pending: a stale snapshot skips rows with a flash in
//! flight, and a confirmed flash re-asserts its fields over a raced write.

use std::sync::{Arc, Mutex};

use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_diorama::{Cache, ChangeFlash, Dio, FlashOutcome, Lens, LoaderCx};

use super::{items, vista_over};
use crate::support::flash::{cached, seeded_dio, staged};
use crate::support::{record, text};

/// A Lens whose writer waits for a permit from `gate`, pinning the pending
/// window open. The writer hands its `LoaderCx` to `seen`.
fn gated_lens(gate: Arc<Semaphore>, seen: Arc<Mutex<Option<LoaderCx>>>) -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::memory())
        .writer(move |cx, _flash| {
            let gate = gate.clone();
            *seen.lock().unwrap() = Some(cx);
            async move {
                gate.acquire().await.expect("gate open").forget();
                Ok(FlashOutcome::applied())
            }
        })
        .build()
        .expect("lens")
}

/// Seed `items()` under `lens` and fire a patch renaming "a" to "edited".
/// Returns once the gated writer holds that patch in the pending window.
async fn edit_a_in_flight(
    lens: &Arc<Lens>,
) -> Result<(Dio, tokio::task::JoinHandle<Result<FlashOutcome>>)> {
    let dio = seeded_dio(lens, vista_over(&items(), "items")).await?;
    let mut bus = dio.subscribe_events();
    let flight = tokio::spawn({
        let dio = dio.clone();
        async move {
            dio.flash(ChangeFlash::partial("a", record(&[("name", "edited")])))
                .await
        }
    });
    staged(&mut bus, &["a"]).await;
    Ok((dio, flight))
}

#[tokio::test]
async fn stale_reconcile_skips_rows_with_a_flash_in_flight() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let lens = gated_lens(gate.clone(), Arc::default());
    let (dio, flight) = edit_a_in_flight(&lens).await?;

    // A reconcile arrives carrying a master snapshot that predates the
    // write: stale for "a", fresh for "b".
    let snapshot = [
        ("a".to_string(), record(&[("id", "a"), ("name", "one")])),
        (
            "b".to_string(),
            record(&[("id", "b"), ("name", "two-updated")]),
        ),
    ];
    dio.reconcile(snapshot.into_iter().collect()).await?;

    let a = cached(&dio, "a").await?.unwrap();
    assert_eq!(
        a.get("name"),
        Some(&text("edited")),
        "the in-flight row must not be clobbered by the stale snapshot"
    );
    let b = cached(&dio, "b").await?.unwrap();
    assert_eq!(
        b.get("name"),
        Some(&text("two-updated")),
        "rows with no flash in flight reconcile normally"
    );

    // Release the write; after it confirms, reconciles apply again.
    gate.add_permits(1);
    flight.await.unwrap()?;
    let later = [(
        "a".to_string(),
        record(&[("id", "a"), ("name", "reconciled-later")]),
    )];
    dio.reconcile(later.into_iter().collect()).await?;
    let a = cached(&dio, "a").await?.unwrap();
    assert_eq!(a.get("name"), Some(&text("reconciled-later")));
    assert!(
        cached(&dio, "b").await?.is_none(),
        "absent from the snapshot"
    );
    Ok(())
}

#[tokio::test]
async fn confirmed_flash_reasserts_its_fields_over_a_raw_stale_write() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let seen = Arc::new(Mutex::new(None));
    let lens = gated_lens(gate.clone(), seen.clone());
    let (dio, flight) = edit_a_in_flight(&lens).await?;

    // A writer that ignores the pending guard clobbers the row mid-flight
    // with a stale snapshot (and a fresh other-field value).
    let cx = seen.lock().unwrap().clone().expect("the writer ran");
    let stale = record(&[("id", "a"), ("name", "one"), ("status", "closed")]);
    cx.cache().insert_value("a", &stale).await?;

    gate.add_permits(1);
    flight.await.unwrap()?;

    // The confirmed flash re-asserts the fields it wrote; fields it never
    // touched keep whatever arrived meanwhile.
    let a = cached(&dio, "a").await?.unwrap();
    assert_eq!(
        a.get("name"),
        Some(&text("edited")),
        "confirmed field re-asserted"
    );
    assert_eq!(
        a.get("status"),
        Some(&text("closed")),
        "untouched field not rolled back"
    );
    Ok(())
}
