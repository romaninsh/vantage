//! A confirmed insert or delete splices the two-pass index in place instead
//! of re-listing it.

use std::time::Duration;

use tempfile::TempDir;
use vantage_diorama::ChangeFlash;
use vantage_vista::mocks::MockShell;

use super::listing::{listed, listings, redb_lens, two_pass};
use crate::support::{self, record, views};

/// Rows `c`, `b`, `a`, in that order: the master's order differs from the
/// redb cache's id order, so a view that re-listed from the cache would read
/// `a, b, c`.
fn reversed() -> MockShell {
    let shell = MockShell::new().with_metadata(support::meta(&["id", "name"]));
    ["c", "b", "a"].into_iter().fold(shell, |shell, id| {
        shell.with_record(id, record(&[("id", id), ("name", id)]))
    })
}

fn zero() -> ChangeFlash {
    ChangeFlash::insert("0", record(&[("id", "0"), ("name", "zero")]))
}

#[tokio::test]
async fn a_confirmed_insert_splices_the_index() {
    let tmp = TempDir::new().unwrap();
    let master = reversed();
    let dio = two_pass(&redb_lens(tmp.path()), &master).await;
    let view = listed(&dio).await;
    assert_eq!(views::ids(&view), ["c", "b", "a"]);
    let before = listings(&master);

    dio.flash(zero()).await.unwrap();

    let spliced = support::eventually(Duration::from_secs(3), || {
        views::ids(&view) == ["c", "b", "a", "0"]
    })
    .await;
    assert!(spliced, "the new row isn't last: {:?}", views::ids(&view));
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(views::ids(&view), ["c", "b", "a", "0"]);
    assert_eq!(listings(&master), before, "the insert re-listed the master");
}

#[tokio::test]
async fn a_confirmed_delete_splices_the_index() {
    let tmp = TempDir::new().unwrap();
    let master = reversed();
    let dio = two_pass(&redb_lens(tmp.path()), &master).await;
    let view = listed(&dio).await;
    let before = listings(&master);

    dio.flash(ChangeFlash::delete("b")).await.unwrap();

    let spliced = support::eventually(Duration::from_secs(3), || {
        views::ids(&view) == ["c", "a"]
    })
    .await;
    assert!(spliced, "the row didn't leave in place: {:?}", views::ids(&view));
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(views::ids(&view), ["c", "a"]);
    assert_eq!(listings(&master), before, "the delete re-listed the master");
}

/// An insert confirmed while the index is still listed waits for the
/// listing: no re-list, and the row shows once, at the end.
#[tokio::test]
async fn an_insert_during_the_listing_waits_for_its_end() {
    let tmp = TempDir::new().unwrap();
    let master = reversed();
    let dio = two_pass(&redb_lens(tmp.path()), &master).await;
    let view = support::open_listed(&dio, 2).await;
    assert_eq!(views::ids(&view), ["c", "b"]);

    dio.flash(zero()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(listings(&master), 1, "the insert re-listed the master");
    view.request_load_more();

    let ended = support::eventually(Duration::from_secs(3), || {
        views::ids(&view) == ["c", "b", "a", "0"]
    })
    .await;
    assert!(ended, "{:?}", views::ids(&view));
    assert_eq!(listings(&master), 2);
}
