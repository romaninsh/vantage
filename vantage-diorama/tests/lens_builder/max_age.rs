//! The `max_age` check: a stale or unstamped cache refreshes on build and on
//! each open, a fresh one is left alone, and a detached eager start counts
//! as the fresh load.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, Lens, Loader, MemoryCache};

use crate::{TTL, build, counting_lens, counting_refresher, refreshed, refreshed_at, stamp};

/// Let every runnable task finish: under a paused clock a sleep completes only
/// once nothing else can run.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(1)).await;
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

#[tokio::test(start_paused = true)]
async fn max_age_without_every_refreshes_a_stale_cache_on_open() -> Result<()> {
    let (refresher, mut rx) = counting_refresher();
    let backend = Arc::new(MemoryCache::new());
    let lens = counting_lens(backend.clone(), refresher.max_age(TTL));
    let dio = build(&lens).await;
    assert!(
        refreshed(&mut rx, Duration::from_secs(1)).await,
        "unstamped cache refreshes on build"
    );

    settle().await;
    let _table = dio.table_scenery().open().await?;
    assert!(
        !refreshed(&mut rx, Duration::from_secs(1)).await,
        "a fresh stamp is left alone"
    );

    // No timer: time passing alone refreshes nothing.
    assert!(
        !refreshed(&mut rx, Duration::from_secs(600)).await,
        "no timer without every"
    );
    stamp(&backend, TTL * 2).await?;
    let _record = dio.record_scenery("a").open().await?;
    assert!(
        refreshed(&mut rx, Duration::from_secs(1)).await,
        "the stamp has outlived max_age at the next open"
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
