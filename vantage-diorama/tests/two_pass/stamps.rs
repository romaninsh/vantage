//! What records a whole copy, and what must not: only a cache that holds
//! the master's whole set, and nothing outside it, is stamped.

use std::sync::Arc;
use std::time::Duration;

use vantage_diorama::{Cache, CacheBackend, Lens, MemoryCache, Tuning};
use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;

use super::listing::{hourly, listed, listings, two_pass};
use crate::support::{self, meta, record, views};

/// A paged plain Dio caches only the windows it was asked for, so its
/// refresh records the time and no copy. An augmented Dio on the same Lens
/// shares its cache table and must not serve its list pass from it.
#[tokio::test]
async fn a_paged_refresh_leaves_no_copy_for_an_augmented_dio() {
    let backend = Arc::new(MemoryCache::new());
    let lens = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .loader(support::page_master())
        .refresher(hourly())
        .tuning(Tuning::default().with_viewport_debounce(Duration::from_millis(1)))
        .build()
        .expect("lens builds");
    let master = views::shell();
    let plain = lens
        .dio(Vista::new("files", master.clone()))
        .build()
        .await
        .unwrap();
    let window = plain.table_scenery().page_size(1).open().await.unwrap();
    window.set_viewport(0..1, 0..1);
    let paged = support::eventually(Duration::from_secs(3), || window.row(0).is_some()).await;
    assert!(paged, "the window never loaded");
    plain.refresh().await.unwrap();
    let table = backend.open_table("files").await.unwrap();
    assert!(table.meta_refreshed_at().await.unwrap().is_some());
    assert_eq!(table.meta_copied_at().await.unwrap(), None);
    let before = listings(&master);

    let augmented = two_pass(&lens, &master).await;
    let view = listed(&augmented).await;

    assert_eq!(views::ids(&view), ["a", "b", "c"]);
    assert!(
        listings(&master) > before,
        "the augmented Dio read the windows"
    );
}

/// `a` is cached from an earlier read and gone upstream: the listing that
/// stamps the copy drops it first.
#[tokio::test]
async fn a_stamped_copy_drops_rows_that_vanished_upstream() {
    let backend = Arc::new(MemoryCache::new());
    let table = backend.open_table("files").await.unwrap();
    table
        .insert_value("x", &record(&[("id", "x"), ("name", "gone")]))
        .await
        .unwrap();
    let master = views::shell();
    let dio = two_pass(&super::listing::lens_on(&backend, false), &master).await;

    listed(&dio).await;

    let held: Vec<String> = table.list_values().await.unwrap().into_keys().collect();
    assert_eq!(held, ["a", "b", "c"]);
    assert!(table.meta_copied_at().await.unwrap().is_some());
}

fn named(shell: MockShell, ids: &[&str]) -> MockShell {
    for id in ids {
        shell.set_record(*id, record(&[("id", id), ("name", id)]));
    }
    shell
}

/// A listing ended because a page added nothing new is a guess, not the
/// whole set. Two rows land ahead of the first page between page reads, so
/// the second page repeats `a, b` and ends the listing at two of six rows.
#[tokio::test]
async fn a_listing_ended_on_a_guess_stamps_no_copy() {
    let backend = Arc::new(MemoryCache::new());
    let master = named(
        MockShell::new().with_metadata(meta(&["id", "name"])),
        &["a", "b", "c", "d"],
    );
    let dio = two_pass(&super::listing::lens_on(&backend, false), &master).await;
    let view = support::open_listed(&dio, 2).await;
    assert_eq!(views::ids(&view), ["a", "b"]);
    master.clear_records();
    named(master.clone(), &["x", "y", "a", "b", "c", "d"]);

    view.request_load_more();
    let ended = support::eventually(Duration::from_secs(3), || !view.has_more()).await;
    assert!(ended, "the repeated page never ended the listing");
    assert_eq!(views::ids(&view), ["a", "b"]);

    let table = backend.open_table("files").await.unwrap();
    assert_eq!(table.meta_copied_at().await.unwrap(), None);
    let held = table.list_values().await.unwrap().len();
    assert_eq!(held, 2, "nothing outside the guess was pruned or added");
}

/// Pages of a live master drift: `a` goes upstream after the first page, so
/// the second page at offset 2 reads `d` alone and the listing misses `c`.
/// Spread over two reads, the listing is no copy, and `c`, held from an
/// earlier read, stays cached.
#[tokio::test]
async fn a_listing_over_two_pages_stamps_and_prunes_nothing() {
    let backend = Arc::new(MemoryCache::new());
    let table = backend.open_table("files").await.unwrap();
    table
        .insert_value("c", &record(&[("id", "c"), ("name", "c")]))
        .await
        .unwrap();
    let master = named(
        MockShell::new().with_metadata(meta(&["id", "name"])),
        &["a", "b", "c", "d"],
    );
    let dio = two_pass(&super::listing::lens_on(&backend, false), &master).await;
    let view = support::open_listed(&dio, 2).await;
    assert_eq!(views::ids(&view), ["a", "b"]);
    master.remove_record("a");

    view.request_load_more();
    let ended = support::eventually(Duration::from_secs(3), || !view.has_more()).await;
    assert!(ended, "the short page never ended the listing");
    assert_eq!(views::ids(&view), ["a", "b", "d"]);

    assert_eq!(table.meta_copied_at().await.unwrap(), None);
    assert!(
        table.get_value("c").await.unwrap().is_some(),
        "c was pruned"
    );
}
