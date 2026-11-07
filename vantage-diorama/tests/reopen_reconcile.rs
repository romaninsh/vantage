//! Rows the master dropped without telling anyone (a script's transactional
//! `DELETE`, a change missed while the app was away) leave a paged view once
//! it reopens: the reopen re-reads the loaded windows in the background and
//! drops the rows that are gone, from the view and from the cache. The rows
//! that stayed keep their places.

mod support;

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Dio, TableScenery};
use vantage_vista::{Capability, Vista, VistaCapabilities, mocks::MockShell};

use support::{eventually, meta, paged_dio, record};

const WITHIN: Duration = Duration::from_secs(3);

fn nominals() -> MockShell {
    ["a", "b", "c", "d", "e", "f"].into_iter().fold(
        MockShell::new()
            .with_metadata(meta(&["id", "name"]))
            .with_capabilities(VistaCapabilities::new().with(Capability::Window)),
        |shell, id| shell.with_record(id, record(&[("id", id), ("name", id)])),
    )
}

fn names(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..scenery.row_count())
        .filter_map(|i| scenery.row(i))
        .filter_map(|row| match row.record.get("name") {
            Some(CborValue::Text(name)) => Some(name.clone()),
            _ => None,
        })
        .collect()
}

async fn cached_ids(dio: &Dio) -> Vec<String> {
    let mut ids: Vec<String> = dio
        .cached()
        .list_values()
        .await
        .map(|rows| rows.into_keys().collect())
        .unwrap_or_default();
    ids.sort();
    ids
}

/// Open a view and wait for its first window: all six rows.
async fn open_loaded(dio: &Dio) -> Arc<dyn TableScenery> {
    let scenery = dio.table_scenery().open().await.expect("view opens");
    scenery.set_viewport(0..6, 0..0);
    assert!(
        eventually(WITHIN, || names(&scenery).len() == 6).await,
        "loaded {:?}",
        names(&scenery)
    );
    scenery
}

/// Wait until the view shows `want` and the cache holds exactly it.
async fn settles_on(dio: &Dio, scenery: &Arc<dyn TableScenery>, want: &[&str]) -> bool {
    tokio::time::timeout(WITHIN, async {
        while names(scenery) != want
            || scenery.status_summary().loaded != want.len()
            || cached_ids(dio).await != want
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok()
}

/// Another view still holds the rows, so the reopen attaches to them; its
/// first viewport re-pulls their windows, which drops the two vanished rows.
#[tokio::test]
async fn a_reopen_beside_a_live_view_drops_rows_deleted_without_a_notification() {
    let shell = nominals();
    let dio = paged_dio(Vista::new("nominal", shell.clone())).await;
    let kept = open_loaded(&dio).await;

    shell.remove_record("b");
    shell.remove_record("e");
    let reopened = dio.table_scenery().open().await.expect("view reopens");
    assert_eq!(names(&reopened).len(), 6, "the reopen paints what it holds");
    reopened.set_viewport(0..6, 0..0);

    assert!(
        settles_on(&dio, &reopened, &["a", "c", "d", "f"]).await,
        "view {:?}, cache {:?}",
        names(&reopened),
        cached_ids(&dio).await
    );
    assert_eq!(names(&kept), ["a", "c", "d", "f"]);
}

/// A paged loader that serves at most `cap` rows per response and states no
/// total: a REST server with a hard page limit.
fn capped_pages(cap: usize) -> vantage_diorama::Loader {
    vantage_diorama::Loader::paged(move |cx, range, query, sink| async move {
        let page = cx.fetch(range.clone(), &query).await?.page;
        for (i, (id, record)) in page.rows.into_iter().take(cap).enumerate() {
            sink.push(range.start + i, id, record).await?;
        }
        Ok(())
    })
}

/// A warm reopen re-pulls several pages; a source that caps each response
/// at one page and states no total must not read the first page back as the
/// whole set and drop the rest from the cache.
#[tokio::test]
async fn a_warm_reopen_over_a_capped_source_drops_nothing_that_exists() {
    let ids: Vec<String> = (0..20).map(|i| format!("r{i:02}")).collect();
    let shell = ids.iter().fold(
        MockShell::new()
            .with_metadata(meta(&["id", "name"]))
            .with_capabilities(VistaCapabilities::new().with(Capability::Window)),
        |shell, id| shell.with_record(id, record(&[("id", id), ("name", id)])),
    );
    let dio = support::memory_lens(capped_pages(5))
        .dio(Vista::new("capped", shell))
        .build()
        .await
        .expect("dio builds");

    let first = dio
        .table_scenery()
        .page_size(5)
        .open()
        .await
        .expect("view opens");
    for start in (0..20).step_by(5) {
        first.set_viewport(start..start + 5, 0..0);
        assert!(
            eventually(WITHIN, || names(&first).len() >= start + 5).await,
            "loaded {:?}",
            names(&first)
        );
    }
    drop(first);
    assert_eq!(cached_ids(&dio).await, ids);

    let reopened = dio
        .table_scenery()
        .page_size(5)
        .open()
        .await
        .expect("view reopens");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(cached_ids(&dio).await, ids, "the warm re-pull dropped rows");
    assert_eq!(names(&reopened).len(), 20, "the view was cut short");
}

/// Views opening together beside a live one share its rows; the re-pull
/// their first viewports ask for runs once, not once per view.
#[tokio::test]
async fn views_opening_together_re_pull_the_shared_rows_once() {
    let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = fetches.clone();
    let loader = vantage_diorama::Loader::paged(move |cx, range, query, sink| {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        async move {
            let page = cx.fetch(range.clone(), &query).await?.page;
            if let Some(total) = page.total {
                sink.set_total(total.max(0) as usize);
            }
            for (i, (id, record)) in page.rows.into_iter().enumerate() {
                sink.push(range.start + i, id, record).await?;
            }
            Ok(())
        }
    });
    let dio = support::memory_lens(loader)
        .dio(Vista::new("nominal", nominals()))
        .build()
        .await
        .expect("dio builds");
    let _kept = open_loaded(&dio).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let before = fetches.load(std::sync::atomic::Ordering::SeqCst);

    let mut views = Vec::new();
    for _ in 0..5 {
        let view = dio.table_scenery().open().await.expect("view opens");
        view.set_viewport(0..6, 0..0);
        views.push(view);
        // Past the viewport debounce, so the loop can't fold the asks.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        fetches.load(std::sync::atomic::Ordering::SeqCst) - before,
        1,
        "one re-pull for five views"
    );
}

/// The last view closed, so the reopen seeds new rows from the warm cache;
/// the reconcile drops the vanished rows from the cache as well.
#[tokio::test]
async fn a_reopen_over_a_warm_cache_drops_rows_deleted_without_a_notification() {
    let shell = nominals();
    let dio = paged_dio(Vista::new("nominal", shell.clone())).await;
    drop(open_loaded(&dio).await);

    shell.remove_record("b");
    shell.remove_record("e");
    let reopened = dio.table_scenery().open().await.expect("view reopens");

    assert!(
        settles_on(&dio, &reopened, &["a", "c", "d", "f"]).await,
        "view {:?}, cache {:?}",
        names(&reopened),
        cached_ids(&dio).await
    );
}
