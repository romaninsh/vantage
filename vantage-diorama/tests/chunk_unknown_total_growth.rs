//! A window-paged source that never states a total.
//!
//! Offset-paginated APIs without a count endpoint exist everywhere: they
//! serve any `[offset, limit)` you ask for and tell you nothing about how
//! many rows there are. The only honest presentation is a list that grows as
//! you scroll — a full page means "there may be more", a short page means
//! "that was the end".
//!
//! Before the horizon rule, the first full page pinned the inferred total at
//! its own size: `row_count` equalled the loaded rows, the viewport could
//! never reach past them, and no scroll could ever request more — the set
//! froze at one page.

use std::sync::Arc;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Lens, Loader, TableScenery};
use vantage_types::Record;

mod support;
use support::chunk::{master as master_cols, redb_lens};

/// What the backend actually holds. Deliberately NOT a multiple of any page
/// size in play, so the exact end arrives as a short page.
const REAL_TOTAL: usize = 133;

fn rec(v: usize) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(format!("v{v}")));
    r
}

/// Paged lens that never calls `set_total` — the source serves windows and
/// states nothing, ever.
fn total_less_lens(cache: std::path::PathBuf) -> Arc<Lens> {
    let rows: Vec<(String, Record<CborValue>)> = (0..REAL_TOTAL)
        .map(|i| (format!("id{i:04}"), rec(i)))
        .collect();
    let loader = Loader::paged(move |_cx, range, _query, sink| {
        let rows = rows.clone();
        async move {
            for idx in range {
                if let Some((id, r)) = rows.get(idx) {
                    sink.push(idx, id.clone(), r.clone()).await?;
                }
            }
            Ok(())
        }
    });
    redb_lens(cache, loader)
}

async fn wait_for(what: &str, scenery: &Arc<dyn TableScenery>, mut ok: impl FnMut() -> bool) {
    let mut rx = scenery.subscribe();
    for _ in 0..100 {
        if ok() {
            return;
        }
        let _ = tokio::time::timeout(std::time::Duration::from_millis(100), rx.changed()).await;
    }
    panic!("timed out waiting for: {what}");
}

/// A grid-faithful scroll: overlapping ~21-row viewports stepping down in
/// small increments, exactly what wheel-scrolling a `RecordGridDio` emits.
/// The set must keep growing under this motion until the real end pins it —
/// no wall partway, whatever the interplay of cached runs, phantom pages
/// and horizon extensions.
#[tokio::test(flavor = "multi_thread")]
async fn grid_step_scroll_reaches_the_exact_end() -> Result<()> {
    const VIEW: usize = 21;
    let tmp = TempDir::new().unwrap();
    let lens = total_less_lens(tmp.path().join("c.redb"));
    let dio = lens.dio(master_cols(&[("v", "String")])).build().await?;
    let scenery = dio.table_scenery().open().await?;

    wait_for("first rows", &scenery, || scenery.row_count() > 0).await;

    let mut top = 0usize;
    for _ in 0..400 {
        let rows = scenery.row_count();
        if rows == REAL_TOTAL && scenery.row(REAL_TOTAL - 1).is_some() {
            assert_eq!(scenery.estimated_total(), Some(REAL_TOTAL));
            return Ok(());
        }
        // One wheel notch down, clamped like a real grid to its content.
        top = (top + 7).min(rows.saturating_sub(VIEW));
        scenery.set_viewport(top..(top + VIEW).min(rows));
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
    }
    panic!(
        "scroll never reached the end: rows={} total={:?}",
        scenery.row_count(),
        scenery.estimated_total()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn full_pages_extend_the_horizon_until_a_short_page_ends_it() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = total_less_lens(tmp.path().join("c.redb"));
    let dio = lens.dio(master_cols(&[("v", "String")])).build().await?;
    let scenery = dio.table_scenery().open().await?;

    // The opening fetch returns a full page; the advertised set must reach
    // PAST the loaded rows, or no scroll can ever ask for more.
    wait_for("first page + horizon", &scenery, || {
        let n = scenery.row_count();
        n > 0 && scenery.row(n - 1).is_none()
    })
    .await;
    assert!(
        scenery.has_more(),
        "a total-less source with a full first page has more"
    );

    // Scroll to the horizon until the set stops growing; each round must
    // load rows the previous horizon exposed.
    let mut last = 0;
    for round in 0..64 {
        let n = scenery.row_count();
        if n == last && round > 0 {
            break;
        }
        last = n;
        scenery.set_viewport(n.saturating_sub(20)..n);
        wait_for("horizon rows load or end", &scenery, || {
            scenery.row_count() != n || scenery.row(n.saturating_sub(1)).is_some()
        })
        .await;
        // Let a follow-up horizon extension land before sampling.
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        if scenery.row_count() == REAL_TOTAL {
            break;
        }
    }

    // The short page pinned the exact end.
    wait_for("exact total", &scenery, || {
        scenery.row_count() == REAL_TOTAL
    })
    .await;
    assert_eq!(scenery.estimated_total(), Some(REAL_TOTAL));

    // And every advertised row is real — no phantom tail left behind.
    wait_for("tail hydrated", &scenery, || {
        scenery.row(REAL_TOTAL - 1).is_some()
    })
    .await;

    Ok(())
}

type Backend = Arc<std::sync::Mutex<Vec<(String, Record<CborValue>)>>>;
type Fetches = Arc<std::sync::Mutex<Vec<std::ops::Range<usize>>>>;

/// A total-less lens over `backend` that records every range it is asked for.
fn recording_lens(cache: std::path::PathBuf, backend: Backend, fetches: Fetches) -> Arc<Lens> {
    let loader = Loader::paged(move |_cx, range, _query, sink| {
        let rows = backend.lock().unwrap().clone();
        fetches.lock().unwrap().push(range.clone());
        async move {
            for idx in range {
                if let Some((id, r)) = rows.get(idx) {
                    sink.push(idx, id.clone(), r.clone()).await?;
                }
            }
            Ok(())
        }
    });
    redb_lens(cache, loader)
}

/// A short page proves where the set ends. A viewport reaching that end must
/// not probe past it, but a refresh still re-fetches, so rows added later
/// are picked up.
#[tokio::test(flavor = "multi_thread")]
async fn a_short_page_end_is_not_probed_past() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend: Backend = Arc::new(std::sync::Mutex::new(
        (0..6).map(|i| (format!("id{i:04}"), rec(i))).collect(),
    ));
    let fetches: Fetches = Arc::default();
    let lens = recording_lens(tmp.path().join("c.redb"), backend.clone(), fetches.clone());
    let dio = lens.dio(master_cols(&[("v", "String")])).build().await?;
    let scenery = dio.table_scenery().page_size(10).open().await?;

    wait_for("short first page", &scenery, || {
        scenery.estimated_total() == Some(6) && scenery.row(5).is_some()
    })
    .await;
    let opened = fetches.lock().unwrap().len();

    scenery.set_viewport(0..6);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let past_end: Vec<_> = fetches.lock().unwrap()[opened..]
        .iter()
        .filter(|r| r.start >= 6)
        .cloned()
        .collect();
    assert!(
        past_end.is_empty(),
        "probed past a proven end: {past_end:?}"
    );
    assert_eq!(scenery.row_count(), 6);

    backend
        .lock()
        .unwrap()
        .extend((6..9).map(|i| (format!("id{i:04}"), rec(i))));
    // The refresh re-fetches the rows on screen; a full page reopens the
    // horizon, and scrolling into it finds the new end.
    scenery.request_refresh();
    wait_for("refresh reopens the horizon", &scenery, || {
        scenery.row_count() > 6
    })
    .await;
    scenery.set_viewport(6..scenery.row_count());
    wait_for("growth loaded to its new end", &scenery, || {
        scenery.row_count() == 9 && scenery.row(8).is_some()
    })
    .await;
    Ok(())
}
