//! `Lens::builder()`: every setting it refuses, and the refresh timing it
//! sets up — the `every` timer and the `max_age` check on build and open.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use tokio::sync::mpsc;
use vantage_core::{Result, VantageError};
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{
    Cache, CacheBackend, Dio, DioEvent, Lens, LensBuilder, Loader, MemoryCache, Refresher, Tuning,
};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Capability, FilterOp, TableShell, Vista};

use support::{cond, copy_master, memory_lens, meta, record, teams_master};

const TTL: Duration = Duration::from_secs(60);

/// The setting a build error names, as the error's `setting` context.
fn setting(err: &VantageError) -> Option<&str> {
    err.context.get("setting").map(|s| s.trim_matches('"'))
}

fn refused(builder: LensBuilder) -> VantageError {
    match builder.build() {
        Ok(_) => panic!("the build was accepted"),
        Err(err) => err,
    }
}

fn memory() -> LensBuilder {
    Lens::builder().cache(Cache::memory())
}

fn noop_loader() -> Loader {
    Loader::paged(|_cx, _range, _query, _sink| async { Ok(()) })
}

#[tokio::test]
async fn a_lens_without_a_cache_is_refused() {
    assert_eq!(setting(&refused(Lens::builder())), Some("cache"));
}

#[test]
fn a_lens_outside_a_runtime_is_refused_not_panicked() {
    assert_eq!(setting(&refused(memory())), Some("runtime"));
}

#[test]
fn an_explicit_runtime_builds_outside_one() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert!(memory().runtime(runtime.handle().clone()).build().is_ok());
}

#[tokio::test]
async fn blocking_on_a_paged_loader_without_an_eager_start_is_refused() {
    let err = refused(memory().loader(noop_loader().blocking(true)));
    assert_eq!(setting(&err), Some("loader.blocking"));

    // With an eager start for the masters that can't page, it applies.
    let hybrid = noop_loader()
        .or_eager(|_cx| async { Ok(()) })
        .blocking(false);
    assert!(memory().loader(hybrid).build().is_ok());
}

#[tokio::test]
async fn or_eager_on_an_eager_loader_is_refused() {
    let loader = Loader::eager(|_cx| async { Ok(()) }).or_eager(|_cx| async { Ok(()) });
    assert_eq!(
        setting(&refused(memory().loader(loader))),
        Some("loader.or_eager")
    );
}

#[tokio::test]
async fn refresher_settings_are_validated() {
    let refresh = || Refresher::new(|_cx| async { Ok(()) });
    let err = refused(memory().refresher(refresh().max_age(TTL)));
    assert_eq!(setting(&err), Some("refresher.max_age"));
    let err = refused(memory().refresher(refresh().every(Duration::ZERO)));
    assert_eq!(setting(&err), Some("refresher.every"));
}

#[tokio::test]
async fn zero_sized_tuning_is_refused() {
    let err = refused(memory().tuning(Tuning::default().with_write_queue(0)));
    assert_eq!(setting(&err), Some("tuning.write_queue"));
    let err = refused(memory().tuning(Tuning::default().with_augment_workers(0)));
    assert_eq!(setting(&err), Some("tuning.augment_workers"));
}

#[tokio::test]
async fn a_redb_file_that_fails_to_open_fails_the_build() {
    // A directory can't be opened as a database file.
    let dir = tempfile::TempDir::new().unwrap();
    let err = refused(Lens::builder().cache(Cache::redb(dir.path())));
    assert_eq!(setting(&err), Some("cache"));
    assert!(
        std::error::Error::source(&err).is_some(),
        "redb's error is kept as the cause: {err}"
    );
}

// ---- refresh timing ------------------------------------------------------

/// A Lens over `backend` whose refresher reports each run on the channel.
fn counting_lens(backend: Arc<MemoryCache>, refresher: Refresher) -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::backend(backend))
        .refresher(refresher)
        .build()
        .expect("lens builds")
}

fn counting_refresher() -> (Refresher, mpsc::UnboundedReceiver<()>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let refresher = Refresher::new(move |_cx| {
        let tx = tx.clone();
        async move {
            let _ = tx.send(());
            Ok(())
        }
    });
    (refresher, rx)
}

/// Whether a refresh runs within `wait`. Under a paused clock the wait
/// elapses as soon as every task is idle.
async fn refreshed(rx: &mut mpsc::UnboundedReceiver<()>, wait: Duration) -> bool {
    tokio::time::timeout(wait, rx.recv()).await.is_ok()
}

/// Let every runnable task finish: under a paused clock a sleep completes only
/// once nothing else can run.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(1)).await;
}

async fn stamp(backend: &MemoryCache, age: Duration) -> Result<()> {
    let table = backend.open_table("members").await?;
    table.set_meta_refreshed_at(SystemTime::now() - age).await
}

/// When the `members` table in `backend` last recorded a refresh.
async fn refreshed_at(backend: &MemoryCache) -> Result<Option<SystemTime>> {
    backend
        .open_table("members")
        .await?
        .meta_refreshed_at()
        .await
}

async fn build(lens: &Arc<Lens>) -> Dio {
    lens.dio(teams_master()).build().await.expect("dio builds")
}

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
async fn max_age_refreshes_a_stale_or_unstamped_cache_on_build() -> Result<()> {
    for age in [Some(TTL * 2), None] {
        let backend = Arc::new(MemoryCache::new());
        if let Some(age) = age {
            stamp(&backend, age).await?;
        }
        let (refresher, mut rx) = counting_refresher();
        let lens = counting_lens(backend.clone(), refresher.every(TTL * 10).max_age(TTL));
        let _dio = build(&lens).await;

        assert!(
            refreshed(&mut rx, Duration::from_secs(1)).await,
            "age {age:?}"
        );
        // The refresh records itself, so the next open finds the cache fresh.
        settle().await;
        let at = refreshed_at(&backend).await?.expect("stamped");
        assert!(SystemTime::now().duration_since(at).unwrap_or_default() < TTL);
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn max_age_leaves_a_fresh_cache_alone() -> Result<()> {
    let backend = Arc::new(MemoryCache::new());
    stamp(&backend, TTL / 2).await?;
    let (refresher, mut rx) = counting_refresher();
    let lens = counting_lens(backend, refresher.every(TTL * 10).max_age(TTL));
    let dio = build(&lens).await;
    let _scenery = dio.table_scenery().open().await?;

    assert!(!refreshed(&mut rx, Duration::from_secs(1)).await);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn max_age_is_checked_again_when_a_scenery_opens() -> Result<()> {
    let backend = Arc::new(MemoryCache::new());
    stamp(&backend, Duration::ZERO).await?;
    let (refresher, mut rx) = counting_refresher();
    let lens = counting_lens(backend.clone(), refresher.every(TTL * 10).max_age(TTL));
    let dio = build(&lens).await;
    assert!(
        !refreshed(&mut rx, Duration::from_secs(1)).await,
        "fresh at build"
    );

    stamp(&backend, TTL * 2).await?;
    let _table = dio.table_scenery().open().await?;
    assert!(
        refreshed(&mut rx, Duration::from_secs(1)).await,
        "table scenery open"
    );

    // Let that refresh record itself before ageing the stamp again.
    settle().await;
    stamp(&backend, TTL * 2).await?;
    let _record = dio.record_scenery("a").open().await?;
    assert!(
        refreshed(&mut rx, Duration::from_secs(1)).await,
        "record scenery open"
    );
    Ok(())
}

/// A detached eager start is the fresh load: a scenery opening while it still
/// copies must not request a second load for `max_age`.
#[tokio::test(start_paused = true)]
async fn max_age_waits_for_a_detached_eager_start() -> Result<()> {
    let backend = Arc::new(MemoryCache::new());
    stamp(&backend, TTL * 2).await?;
    let gate = Arc::new(tokio::sync::Notify::new());
    let opener = gate.clone();
    let start = Loader::eager(move |cx| {
        let opener = opener.clone();
        async move {
            opener.notified().await;
            let rows = cx.master().list_values().await?;
            cx.reconcile(rows).await
        }
    })
    .blocking(false);
    let (refresher, mut rx) = counting_refresher();
    let lens = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .loader(start)
        .refresher(refresher.every(TTL * 10).max_age(TTL))
        .build()
        .expect("lens builds");
    let dio = build(&lens).await;
    let _table = dio.table_scenery().open().await?;

    gate.notify_one();
    settle().await;
    assert!(!refreshed(&mut rx, Duration::from_secs(1)).await);
    let at = refreshed_at(&backend).await?.expect("stamped");
    assert!(SystemTime::now().duration_since(at).unwrap_or_default() < TTL);
    Ok(())
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
    windowed.with_query(&red).await?;
    assert_eq!(starts.load(Ordering::SeqCst), 0, "a Window master pages");

    let shell = MockShell::new()
        .with_record("a", record(&[("id", "a"), ("team", "red")]))
        .with_metadata(meta(&["id", "team"]));
    let caps = shell.capabilities().without(Capability::Window);
    let unwindowed = Vista::new("unwindowed", shell.with_capabilities(caps));
    let copied = lens.dio(unwindowed).build().await?;
    copied.with_query(&red).await?;
    assert_eq!(
        starts.load(Ordering::SeqCst),
        2,
        "the build and the derivation copy it"
    );
    Ok(())
}

/// Every cached row is keyed by the id column, so a vista without one is
/// refused when the Dio is built, naming the table.
#[tokio::test]
async fn a_vista_without_an_id_column_fails_the_build() {
    let master = Vista::new("keyless", MockShell::new());
    let err = memory_lens(copy_master())
        .dio(master)
        .build()
        .await
        .err()
        .expect("the build was accepted");
    assert!(err.to_string().contains("id column"), "{err}");
    assert_eq!(
        err.context.get("table").map(|s| s.trim_matches('"')),
        Some("keyless")
    );
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
