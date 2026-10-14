//! A failed load leaves the rows on screen as the cache holds them, even when
//! it pushed rows before failing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Dio, Lens, Loader};

use super::fixtures::{Backend, push_rows, rec, rows, wait_until};
use crate::support::MockView;
use crate::support::chunk::{master, redb_lens};

/// A lens whose paged loader fails while `fail` is set. `pushes_first`
/// decides whether it fails clean or fails having already bound rows the
/// cache never saw.
fn failing_lens(
    cache: std::path::PathBuf,
    backend: Backend,
    fail: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    pushes_first: bool,
) -> Arc<Lens> {
    let loader = Loader::paged(move |_cx, range, _query, sink| {
        let b = backend.clone();
        let fail = fail.clone();
        let calls = calls.clone();
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            if fail.load(Ordering::SeqCst) {
                if pushes_first {
                    // Two rows with genuinely new values, so they bind
                    // rather than dedup away, and then the fetch dies.
                    for idx in range.clone().take(2) {
                        sink.push(idx, format!("r{idx}"), rec(&format!("v{idx}-new")))
                            .await?;
                    }
                }
                return Err(vantage_core::error!("API request failed", status = 503));
            }
            let rows = b.lock().unwrap().clone();
            sink.set_total(rows.len());
            push_rows(&rows, range, &sink).await
        }
    });
    redb_lens(cache, loader)
}

/// The first page loaded, then a refresh that failed. `before` is column `v`
/// of the first ten rows as they were on screen before the refresh.
struct FailedRefresh {
    _tmp: TempDir,
    _dio: Dio,
    view: MockView,
    before: Vec<Option<String>>,
}

impl FailedRefresh {
    async fn run(pushes_first: bool) -> Result<Self> {
        let tmp = TempDir::new().unwrap();
        let fail = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(AtomicUsize::new(0));
        let lens = failing_lens(
            tmp.path().join("c.redb"),
            rows(30),
            fail.clone(),
            calls.clone(),
            pushes_first,
        );
        let dio = lens.dio(master(&[("v", "String")])).build().await?;
        let view = MockView::open(&dio, 10).await;
        view.settle_until("first page", |v| v.loaded_rows() >= 10)
            .await;
        let before = view_column(&view);
        let calls_before = calls.load(Ordering::SeqCst);

        fail.store(true, Ordering::SeqCst);
        view.scenery().refresh_source();
        // Wait for the failing fetch to have actually run: without this the
        // assertions hold vacuously, over a refresh that never dispatched.
        wait_until("the failing refresh to run", || {
            calls.load(Ordering::SeqCst) > calls_before
        })
        .await;
        Ok(Self {
            _tmp: tmp,
            _dio: dio,
            view,
            before,
        })
    }
}

fn view_column(view: &MockView) -> Vec<Option<String>> {
    (0..10).map(|i| view.col_at(i, "v")).collect()
}

#[tokio::test]
async fn a_failed_background_refresh_keeps_the_rows() -> Result<()> {
    let run = FailedRefresh::run(false).await?;

    assert_eq!(
        run.before,
        view_column(&run.view),
        "rows on screen survive a failed refresh",
    );
    assert_eq!(run.view.gray_rows(), 0);
    Ok(())
}

/// A failure after the callback has already pushed rows unwinds like a
/// cancellation: those rows were never committed to the cache, so leaving
/// them on screen would show values no later read can reproduce.
#[tokio::test]
async fn a_failing_refresh_that_pushed_rows_first_restores_them() -> Result<()> {
    let run = FailedRefresh::run(true).await?;
    let view = &run.view;
    // The restore happens in the commit half, after the callback returns.
    wait_until("rows 0 and 1 to be back", || {
        view.col_at(0, "v").as_deref() == Some("v0") && view.col_at(1, "v").as_deref() == Some("v1")
    })
    .await;

    assert_eq!(
        run.before,
        view_column(view),
        "the rows the failed fetch had pushed went back to their cached values",
    );
    assert_eq!(view.gray_rows(), 0);
    Ok(())
}
