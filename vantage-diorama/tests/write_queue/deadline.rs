//! A master write that never answers: it gives up at the Lens's write
//! deadline, and until then it holds up only its own Dio.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, DioEvent, Lens, Tuning};
use vantage_types::Record;
use vantage_vista::{TableShell, Vista, forward_table_shell};

use super::{items, name_of, rename, seeded, staged};

/// A master whose patches never answer, like a transport retrying an
/// unavailable backend for as long as someone waits.
#[derive(Clone)]
struct Stuck {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Stuck, inner, {
    async fn patch(&self, _id: &str, _partial: &Record<CborValue>) -> Result<Record<CborValue>> {
        std::future::pending().await
    }
});

fn lens_with_deadline(deadline: Duration) -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::memory())
        .tuning(Tuning::default().with_write_deadline(deadline))
        .build()
        .expect("lens builds")
}

#[tokio::test(start_paused = true)]
async fn a_write_past_its_deadline_fails_unavailable_and_rolls_back() -> Result<()> {
    let lens = lens_with_deadline(Duration::from_millis(50));
    let master = Vista::new("items", items()).map_shell(|inner| Stuck { inner });
    let dio = seeded(&lens, master).await?;
    let mut bus = dio.subscribe_events();

    let err = dio.flash(rename("lost")).await.unwrap_err();
    assert!(err.is_unavailable(), "{err}");
    assert!(err.to_string().contains("gave up"), "{err}");
    let row = dio.cached().get_value("a").await?;
    assert_eq!(name_of(row).as_deref(), Some("one"), "rolled back");
    let reverted = std::iter::from_fn(|| bus.try_recv().ok())
        .any(|e| matches!(e, DioEvent::WriteReverted { .. }));
    assert!(reverted, "the rollback is announced");
    Ok(())
}

/// Each Dio has its own write worker: a write stuck on one master leaves
/// another Dio's writes free to land.
#[tokio::test]
async fn a_stuck_write_does_not_hold_up_another_dio() -> Result<()> {
    let lens = lens_with_deadline(Duration::from_secs(180));
    let stuck_master = Vista::new("items", items()).map_shell(|inner| Stuck { inner });
    let stuck = seeded(&lens, stuck_master).await?;
    let free = seeded(&lens, Vista::new("others", items())).await?;

    let mut bus = stuck.subscribe_events();
    let held = tokio::spawn({
        let stuck = stuck.clone();
        async move { stuck.flash(rename("waits")).await }
    });
    staged(&mut bus).await;

    tokio::time::timeout(Duration::from_secs(1), free.flash(rename("lands")))
        .await
        .expect("the other Dio's write settles")?;
    let row = free.master().get_value("a").await?;
    assert_eq!(name_of(row).as_deref(), Some("lands"));
    assert!(!held.is_finished(), "the stuck write is still waiting");
    held.abort();
    Ok(())
}
