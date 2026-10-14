//! Which priority each kind of load runs under.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;
use vantage_core::{Priority, Result};
use vantage_diorama::{Cache, Lens, Loader, Tuning};

use super::fixtures::{Backend, push_rows, rows, wait_until};
use crate::support::MockView;
use crate::support::chunk::{master, searchable_master};

/// A paged lens whose loader records the priority it ran under. `with_total`
/// and `debounce` are separate knobs because two rules only show themselves
/// without one or with a longer other: the horizon probe needs a source that
/// never states a total, and the coalescing rule needs a debounce window wide
/// enough to put two requests into by hand.
fn observing_lens(
    cache: std::path::PathBuf,
    backend: Backend,
    seen: Arc<Mutex<Vec<Priority>>>,
) -> Arc<Lens> {
    build_observing_lens(cache, backend, seen, true, None)
}

fn build_observing_lens(
    cache: std::path::PathBuf,
    backend: Backend,
    seen: Arc<Mutex<Vec<Priority>>>,
    with_total: bool,
    debounce: Option<Duration>,
) -> Arc<Lens> {
    let loader = Loader::paged(move |_cx, range, _query, sink| {
        let b = backend.clone();
        let seen = seen.clone();
        async move {
            seen.lock().unwrap().push(Priority::current());
            let rows = b.lock().unwrap().clone();
            if with_total {
                sink.set_total(rows.len());
            }
            push_rows(&rows, range, &sink).await
        }
    });
    let mut tuning = Tuning::default();
    if let Some(debounce) = debounce {
        tuning = tuning.with_viewport_debounce(debounce);
    }
    Lens::builder()
        .cache(Cache::redb(cache))
        .loader(loader)
        .tuning(tuning)
        .build()
        .expect("lens")
}

#[tokio::test]
async fn cold_open_and_uncached_viewport_are_essential_refresh_is_background() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(tmp.path().join("c.redb"), rows(30), seen.clone());
    let dio = lens.dio(master(&[("v", "String")])).build().await?;

    // Cold open: the cache is empty, so the on-open fetch is essential.
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    assert_eq!(seen.lock().unwrap().as_slice(), [Priority::Essential]);

    // Scrolling into rows nobody has cached: essential.
    view.viewport(20..30);
    view.settle_until("second page", |v| v.col_at(25, "v").is_some())
        .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Essential));

    // A refresh over rows already on screen: background.
    view.scenery().refresh_source();
    let before = seen.lock().unwrap().len();
    wait_until("refresh ran", || seen.lock().unwrap().len() > before).await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Background));
    Ok(())
}

#[tokio::test]
async fn load_more_is_essential() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(tmp.path().join("c.redb"), rows(30), seen.clone());
    let dio = lens.dio(master(&[("v", "String")])).build().await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    let before = seen.lock().unwrap().len();
    view.scenery().request_load_more();
    wait_until("load more ran", || seen.lock().unwrap().len() > before).await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Essential));
    Ok(())
}

/// A search is a `force_load` re-pull of a block already on screen — and a
/// user waiting on it. `force_load` is not what decides the priority.
#[tokio::test]
async fn a_search_refetch_is_essential() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(tmp.path().join("c.redb"), rows(30), seen.clone());
    let dio = lens
        .dio(searchable_master(&[("v", "String")]))
        .build()
        .await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;

    let before = seen.lock().unwrap().len();
    view.scenery().set_search(Some("v2".into()));
    wait_until("the search refetch ran", || {
        seen.lock().unwrap().len() > before
    })
    .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Essential));
    Ok(())
}

/// The probe past the end the loader itself inferred is speculative: nobody
/// asked for those rows, so it must not hold a retry budget open.
#[tokio::test]
async fn the_horizon_probe_runs_background() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    // No total: nothing ever states one, so the end of the set is only ever
    // this side's inference — which is what makes a viewport reaching it a
    // probe rather than a fetch.
    let lens = build_observing_lens(
        tmp.path().join("c.redb"),
        rows(30),
        seen.clone(),
        false,
        None,
    );
    let dio = lens.dio(master(&[("v", "String")])).build().await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    // The cold on-open fetch is a real wait; the inferred horizon is now 20.
    assert_eq!(seen.lock().unwrap().as_slice(), [Priority::Essential]);
    assert_eq!(view.total(), Some(20));

    // A viewport pressed against that inferred end asks past it on the user's
    // behalf, but the rows beyond it are not known to exist.
    let before = seen.lock().unwrap().len();
    view.viewport(10..20);
    wait_until("the horizon probe ran", || {
        seen.lock().unwrap().len() > before
    })
    .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Background));
    Ok(())
}

/// Coalescing merges the *wait*, not just the range: an essential viewport
/// absorbed together with a background refresh still has someone waiting on
/// the rows.
#[tokio::test]
async fn coalescing_a_refresh_into_a_viewport_keeps_essential() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    // A wide debounce so both requests land in the same window by hand
    // rather than by luck.
    let lens = build_observing_lens(
        tmp.path().join("c.redb"),
        rows(30),
        seen.clone(),
        true,
        Some(Duration::from_millis(400)),
    );
    let dio = lens.dio(master(&[("v", "String")])).build().await?;
    let view = MockView::open(&dio, 10).await;
    view.settle_until("first page", |v| v.loaded_rows() >= 10)
        .await;
    let before = seen.lock().unwrap().len();

    // An essential viewport over the block already on screen — on its own it
    // is fully cached and fetches nothing — then, inside the debounce, the
    // background refresh of the same block, which supplies the `force_load`.
    // One fetch comes out of the two, and it is the one someone is waiting on.
    view.viewport(0..10);
    view.scenery().refresh_source();
    wait_until("the coalesced fetch ran", || {
        seen.lock().unwrap().len() > before
    })
    .await;
    assert_eq!(seen.lock().unwrap().last(), Some(&Priority::Essential));
    Ok(())
}

#[tokio::test]
async fn warm_open_refresh_is_background() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let cache = tmp.path().join("c.redb");
    let backend = rows(30);
    {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let lens = observing_lens(cache.clone(), backend.clone(), seen);
        let dio = lens.dio(master(&[("v", "String")])).build().await?;
        let view = MockView::open(&dio, 10).await;
        view.settle_until("seeded", |v| v.loaded_rows() >= 10).await;
    }
    // Second open on the same cache file: rows are present, so the on-open
    // re-pull is a refresh, not a wait.
    let seen: Arc<Mutex<Vec<Priority>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = observing_lens(cache, backend, seen.clone());
    let dio = lens.dio(master(&[("v", "String")])).build().await?;
    let _view = MockView::open(&dio, 10).await;
    wait_until("re-pulled", || !seen.lock().unwrap().is_empty()).await;
    assert_eq!(seen.lock().unwrap().first(), Some(&Priority::Background));
    Ok(())
}
