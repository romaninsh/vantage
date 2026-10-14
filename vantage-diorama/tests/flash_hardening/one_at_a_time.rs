//! One write at a time: the queue never overlaps a Dio's write-throughs.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_diorama::{Cache, ChangeFlash, FlashOutcome, Lens};

use super::{items, vista_over};
use crate::support::flash::{seeded_dio, staged};
use crate::support::record;

/// Two flashes raced on one Dio: the second write-through starts only once
/// the first has finished, and they land in the order they were queued.
#[tokio::test]
async fn concurrent_flashes_never_overlap_their_write_throughs() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let order = Arc::new(Mutex::new(Vec::new()));
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer({
            let (gate, running, most, order) =
                (gate.clone(), running.clone(), most.clone(), order.clone());
            move |_cx, flash| {
                let (gate, running, most, order) =
                    (gate.clone(), running.clone(), most.clone(), order.clone());
                async move {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    gate.acquire().await.expect("gate open").forget();
                    order.lock().unwrap().push(flash.id().unwrap().to_string());
                    running.fetch_sub(1, Ordering::SeqCst);
                    Ok(FlashOutcome::applied())
                }
            }
        })
        .build()?;
    let dio = seeded_dio(&lens, vista_over(&items(), "items")).await?;
    let mut bus = dio.subscribe_events();

    let first = tokio::spawn({
        let dio = dio.clone();
        async move {
            dio.flash(ChangeFlash::partial("a", record(&[("name", "x")])))
                .await
        }
    });
    staged(&mut bus, &["a"]).await;
    let second = tokio::spawn({
        let dio = dio.clone();
        async move {
            dio.flash(ChangeFlash::partial("b", record(&[("name", "y")])))
                .await
        }
    });
    staged(&mut bus, &["b"]).await;

    gate.add_permits(2);
    first.await.unwrap()?;
    second.await.unwrap()?;
    assert_eq!(most.load(Ordering::SeqCst), 1, "write-throughs overlapped");
    assert_eq!(*order.lock().unwrap(), ["a", "b"]);
    Ok(())
}
