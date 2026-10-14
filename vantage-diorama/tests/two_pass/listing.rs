//! Where a two-pass list pass reads from: the cache once the Dio holds a
//! whole copy, the master otherwise, and one source for a whole listing.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vantage_diorama::{
    Cache, CacheBackend, Dio, Lens, LoadState, MemoryCache, Refresher, TableScenery, Tuning,
};
use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;

use crate::support::{self, views};

pub(crate) const HOUR: Duration = Duration::from_secs(3600);

pub(crate) fn redb_lens(dir: &Path) -> Arc<Lens> {
    support::quick_lens(Cache::redb(dir.join("cache.redb")))
}

/// A Lens over `backend`, so a later Lens over it reads what this one left,
/// as a reopened file would. A refresher, when asked for, never ticks inside
/// a test and holds a copy for an hour.
pub(crate) fn lens_on(backend: &Arc<MemoryCache>, refreshed: bool) -> Arc<Lens> {
    let builder = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .tuning(Tuning::default().with_viewport_debounce(Duration::from_millis(1)));
    let builder = match refreshed {
        true => builder.refresher(hourly()),
        false => builder,
    };
    builder.build().expect("lens builds")
}

/// A refresher that writes nothing, never ticks inside a test, and holds a
/// cache for an hour.
pub(crate) fn hourly() -> Refresher {
    Refresher::new(|_cx| async { Ok(()) })
        .every(HOUR)
        .max_age(HOUR)
}

/// A two-pass Dio over `master` as `files`, augmenting `size` per row. No
/// view sets a viewport here, so the detail source is never read.
pub(crate) async fn two_pass(lens: &Arc<Lens>, master: &MockShell) -> Dio {
    let sizes = support::size_detail([("a", "1"), ("b", "2"), ("c", "3")]);
    support::augmented_dio(
        lens,
        Vista::new("files", master.clone()),
        vec![support::fixed_augmentation(Vista::new("sizes", sizes), &["size"])],
    )
    .await
}

/// An unrefined view whose one page holds the whole set, once it reads
/// `Complete`.
pub(crate) async fn listed(dio: &Dio) -> Arc<dyn TableScenery> {
    let view = dio.table_scenery().page_size(50).open().await.unwrap();
    let complete = support::eventually(Duration::from_secs(3), || {
        matches!(view.load_state(), LoadState::Complete)
    })
    .await;
    assert!(complete, "never Complete: {:?}", view.load_state());
    view
}

pub(crate) fn listings(master: &MockShell) -> usize {
    let calls = master.calls();
    calls.list + calls.fetch_window
}

/// The cache stamped fresh, so building a Dio with a `max_age` refreshes
/// nothing and the test counts only its own listings.
async fn stamped_fresh(backend: &MemoryCache) {
    let table = backend.open_table("files").await.unwrap();
    table.set_meta_refreshed_at(SystemTime::now()).await.unwrap();
}

#[tokio::test]
async fn a_warm_whole_copy_lists_from_the_cache() {
    let backend = Arc::new(MemoryCache::new());
    stamped_fresh(&backend).await;
    let master = views::shell();
    let first = {
        let dio = two_pass(&lens_on(&backend, true), &master).await;
        views::ids(&listed(&dio).await)
    };
    assert_eq!(first, ["a", "b", "c"]);
    let before = listings(&master);

    let dio = two_pass(&lens_on(&backend, true), &master).await;
    let view = listed(&dio).await;

    assert_eq!(views::ids(&view), first);
    assert_eq!(listings(&master), before, "the warm open listed the master");
}

/// With no `max_age`, nothing says how old an earlier session's copy may be.
#[tokio::test]
async fn an_earlier_sessions_copy_without_a_max_age_lists_the_master() {
    let backend = Arc::new(MemoryCache::new());
    let master = views::shell();
    {
        let dio = two_pass(&lens_on(&backend, false), &master).await;
        listed(&dio).await;
    }
    let table = backend.open_table("files").await.unwrap();
    let copied = table.meta_copied_at().await.unwrap();
    assert!(copied.is_some(), "run 1 stamped a copy");
    let before = listings(&master);

    let dio = two_pass(&lens_on(&backend, false), &master).await;
    let view = listed(&dio).await;

    assert_eq!(views::ids(&view), ["a", "b", "c"]);
    assert!(listings(&master) > before, "an untimed old copy served the list");
}

#[tokio::test]
async fn a_cold_cache_still_lists_the_master() {
    let tmp = tempfile::TempDir::new().unwrap();
    let master = views::shell();
    let dio = two_pass(&redb_lens(tmp.path()), &master).await;

    let view = listed(&dio).await;

    assert_eq!(views::ids(&view), ["a", "b", "c"]);
    assert!(listings(&master) > 0, "a cold cache must list the master");
}

/// A listing that began on the master stays on it: the copy stamped after
/// its first page holds only that page, and reading the rest from it would
/// end the listing one row in.
#[tokio::test]
async fn a_copy_stamped_mid_listing_leaves_the_listing_on_the_master() {
    let backend = Arc::new(MemoryCache::new());
    let master = views::shell();
    let dio = two_pass(&lens_on(&backend, false), &master).await;
    let view = support::open_listed(&dio, 1).await;
    assert_eq!(views::ids(&view), ["a"]);
    let table = backend.open_table("files").await.unwrap();
    table.set_meta_copied_at(SystemTime::now()).await.unwrap();

    for upto in [2, 3] {
        view.request_load_more();
        let grew = support::eventually(Duration::from_secs(3), || view.row_count() == upto).await;
        assert!(grew, "stuck at {:?}", views::ids(&view));
    }

    assert_eq!(views::ids(&view), ["a", "b", "c"]);
    assert_eq!(master.calls().fetch_window, 3, "one master page per row");
    assert!(!view.has_more());
}
