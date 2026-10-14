//! The refresher's `every` timer, what a refresh records, and the eager
//! start that loads a Dio (and copies only a master without `Window`).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, DioEvent, Lens, Loader, MemoryCache, Refresher};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Capability, FilterOp, TableShell, Vista};

use crate::support::{cond, copy_master, memory_lens, meta, record, teams_master};
use crate::{TTL, build, counting_lens, counting_refresher, refreshed, refreshed_at, stamp};

#[tokio::test(start_paused = true)]
async fn every_runs_the_refresh_on_a_timer() {
    let (refresher, mut rx) = counting_refresher();
    let lens = counting_lens(Arc::default(), refresher.every(Duration::from_secs(10)));
    let _dio = build(&lens).await;

    assert!(
        !refreshed(&mut rx, Duration::from_secs(9)).await,
        "not before the interval"
    );
    assert!(
        refreshed(&mut rx, Duration::from_secs(2)).await,
        "first tick"
    );
    assert!(
        refreshed(&mut rx, Duration::from_secs(11)).await,
        "second tick"
    );
}

#[tokio::test(start_paused = true)]
async fn a_lens_without_a_refresher_never_ticks() {
    let dio = build(&memory_lens(copy_master())).await;
    let mut events = dio.subscribe_events();
    tokio::time::sleep(Duration::from_secs(3600)).await;
    while let Ok(event) = events.try_recv() {
        assert!(
            !matches!(event, DioEvent::Refreshing),
            "a refresh ran without a refresher"
        );
    }
}

#[tokio::test]
async fn a_failed_refresh_records_no_refresh_time() -> Result<()> {
    let failing = Refresher::new(|_cx| async { Err(vantage_core::error!("source down")) });
    let backend = Arc::new(MemoryCache::new());
    let dio = build(&counting_lens(backend.clone(), failing)).await;
    assert!(dio.refresh().await.is_err());
    assert_eq!(refreshed_at(&backend).await?, None);
    Ok(())
}

/// Under `or_eager`, the eager start runs for a Dio, built or derived, only
/// when its master can't serve windows: a master that can is never copied
/// whole.
#[tokio::test]
async fn or_eager_copies_only_a_master_without_window() -> Result<()> {
    let starts = Arc::new(AtomicUsize::new(0));
    let counted = starts.clone();
    let loader =
        Loader::paged(|_cx, _range, _query, _sink| async { Ok(()) }).or_eager(move |_cx| {
            counted.fetch_add(1, Ordering::SeqCst);
            async { Ok(()) }
        });
    let lens = memory_lens(loader);

    let red = cond("team", FilterOp::Eq, "red");
    let windowed = lens.dio(teams_master()).build().await?;
    windowed.derive().with_query(&red).build().await?;
    assert_eq!(starts.load(Ordering::SeqCst), 0, "a Window master pages");

    let shell = MockShell::new()
        .with_record("a", record(&[("id", "a"), ("team", "red")]))
        .with_metadata(meta(&["id", "team"]));
    let caps = shell.capabilities().without(Capability::Window);
    let unwindowed = Vista::new("unwindowed", shell.with_capabilities(caps));
    let copied = lens.dio(unwindowed).build().await?;
    copied.derive().with_query(&red).build().await?;
    assert_eq!(
        starts.load(Ordering::SeqCst),
        2,
        "the build and the derivation copy it"
    );
    Ok(())
}

#[tokio::test]
async fn an_eager_start_records_the_refresh_time() -> Result<()> {
    let backend = Arc::new(MemoryCache::new());
    let lens = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .loader(copy_master())
        .build()?;
    let dio = build(&lens).await;
    assert_eq!(dio.cached().count().await?, 3);
    assert!(refreshed_at(&backend).await?.is_some());
    Ok(())
}

/// A support refresher hands the set to `LoaderCx::reconcile`: the cache is
/// made to match it and the refresh time is recorded.
#[tokio::test]
async fn a_refresher_reconciles_through_its_loader_cx() -> Result<()> {
    let backend = Arc::new(MemoryCache::new());
    let shell = MockShell::new()
        .with_record("a", record(&[("id", "a"), ("team", "red")]))
        .with_record("b", record(&[("id", "b"), ("team", "blue")]))
        .with_metadata(meta(&["id", "team"]));
    let refresher = Refresher::new(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    });
    let lens = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .loader(copy_master())
        .refresher(refresher)
        .build()?;
    let dio = lens
        .dio(Vista::new("members", shell.clone()))
        .build()
        .await?;
    shell.remove_record("b");
    stamp(&backend, TTL * 2).await?;

    dio.refresh().await?;
    let rows = dio.cached().list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["a"], "b left the set");
    let at = refreshed_at(&backend).await?.expect("stamped");
    assert!(SystemTime::now().duration_since(at).unwrap_or_default() < TTL);
    Ok(())
}
