//! A paged source that promises more rows than it will serve.
//!
//! Real APIs do this. SendLayer's events endpoint reports `TotalRecords: 42`
//! while only ever yielding 35 distinct rows: ask for the tail and it returns
//! ids already held, because it ignores the offset once past what it will hand
//! out. The grid sizes itself to the stated total, so rows 35..41 are holes; a
//! hole is what triggers a fetch; the fetch returns nothing new. Left alone
//! that is an unbounded loop — one request every few seconds, forever, against
//! the slowest dependency in the system — and the user sees a scrollbar that
//! never fills.
//!
//! The scenery has no way to know in advance which sources lie. It finds out
//! the only way anyone could: it asks for a range with holes in it and none of
//! them come back filled.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Lens, SortDir};
use vantage_types::Record;

mod support;
use support::chunk::{master as master_cols, settle};

/// Comfortably past the 50ms viewport debounce — long enough that a fetch this
/// test claims does not happen would have been dispatched by now.
const DEBOUNCE_MARGIN: std::time::Duration = std::time::Duration::from_millis(120);

/// Rows the source will actually hand out, however many it claims to have.
const SERVED: usize = 5;
/// What it claims.
const STATED: usize = 8;

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

fn backend() -> Vec<(String, Record<CborValue>)> {
    (0..SERVED)
        .map(|i| (format!("id{i}"), rec(&format!("v{i}"))))
        .collect()
}

/// A lens over a source that overstates its total.
///
/// Within `SERVED` it pages honestly. Past that it ignores the offset and
/// replies with rows it has already given out — under their original ids, which
/// is what makes them invisible: they overwrite cache entries that exist rather
/// than filling the slots that were asked for. Every response states `STATED`.
fn lying_lens(
    cache: std::path::PathBuf,
    calls: Arc<Mutex<Vec<std::ops::Range<usize>>>>,
) -> Arc<Lens> {
    let rows = backend();
    Lens::new()
        .cache_at(cache)
        .on_load_chunk(move |_dio, range, _sort, sink| {
            let rows = rows.clone();
            let calls = calls.clone();
            async move {
                calls.lock().unwrap().push(range.clone());
                sink.set_total(STATED);
                for idx in range {
                    // Past what it serves, the offset is ignored: the source
                    // repeats a row it already sent, id and all.
                    let (id, record) = &rows[idx.min(SERVED - 1)];
                    sink.push(idx, id.clone(), record.clone()).await?;
                }
                Ok(())
            }
        })
        .build()
        .map(Arc::new)
        .expect("build lens")
}

/// The set is capped at what the source will actually deliver, and the
/// unreachable tail is asked for a bounded number of times — not forever.
#[tokio::test(flavor = "multi_thread")]
async fn overstated_total_does_not_loop() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let calls: Arc<Mutex<Vec<std::ops::Range<usize>>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = lying_lens(tmp.path().join("c.redb"), calls.clone());
    let dio = lens.make_dio(master_cols(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().open().await?;

    // A client-side sort makes the visible map a projection of the cache — the
    // shape this bites in, because duplicate ids collapse there and the slots
    // stay empty however many rows come back.
    scenery.set_sort(Some("v".to_string()), SortDir::Asc);
    settle().await;

    // Look at the whole set the source claims exists.
    scenery.set_viewport(0..STATED);

    // Wait for the outcome, not for a duration. A fixed sleep here is a speed
    // assertion — and the opening fetch writes a row per redb transaction, so
    // "long enough" is not a constant, least of all on a loaded machine running
    // the rest of the suite alongside this.
    wait_for_row_count(&scenery, SERVED).await;
    let settled = calls.lock().unwrap().len();

    // And re-asking for the same viewport must not restart it. Proving an
    // absence does need real time, but only enough to cover the viewport
    // debounce plus a fetch — not the quarter-second `settle` uses to be safe
    // about arrivals, which is the wrong budget for observing nothing.
    for _ in 0..4 {
        scenery.set_viewport(0..STATED);
        tokio::time::sleep(DEBOUNCE_MARGIN).await;
    }
    let after = calls.lock().unwrap().clone();
    assert_eq!(
        after.len(),
        settled,
        "the unreachable tail is being re-fetched: {after:?}",
    );

    Ok(())
}

/// Block until row `idx` is materialised.
async fn wait_for_row(scenery: &Arc<dyn vantage_diorama::TableScenery>, idx: usize) {
    let mut generations = scenery.subscribe();
    let arrived = async {
        while scenery.row(idx).is_none() {
            generations
                .changed()
                .await
                .expect("generation watch closed");
        }
    };
    if tokio::time::timeout(std::time::Duration::from_secs(3), arrived)
        .await
        .is_err()
    {
        panic!("row {idx} never loaded");
    }
}

/// Block until the scenery reports `want` rows.
///
/// Driven by the generation watch, not a poll: every change to the visible set
/// bumps it, so sleeping between checks only adds latency to a signal that
/// already exists. Polling here cost this test two seconds of sleep to observe
/// something that happens in milliseconds.
///
/// The timeout bounds a hang; it does not assert a speed.
async fn wait_for_row_count(scenery: &Arc<dyn vantage_diorama::TableScenery>, want: usize) {
    let mut generations = scenery.subscribe();
    let reached = async {
        while scenery.row_count() != want {
            generations
                .changed()
                .await
                .expect("generation watch closed");
        }
    };
    if tokio::time::timeout(std::time::Duration::from_secs(3), reached)
        .await
        .is_err()
    {
        panic!(
            "row_count settled at {} (wanted {want}) — the set was never capped \
             at what the source will serve",
            scenery.row_count(),
        );
    }
}

/// The clamp keys on holes going unfilled, NOT on a page being shorter than
/// requested — a source that caps its window size still pages correctly, and
/// treating its first short page as the end of the set would strand the user on
/// one screen.
#[tokio::test(flavor = "multi_thread")]
async fn page_capped_source_still_pages() -> Result<()> {
    const CAP: usize = 3;
    const TOTAL: usize = 9;

    let tmp = TempDir::new().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    let lens = Lens::new()
        .cache_at(tmp.path().join("c.redb"))
        .on_load_chunk(move |_dio, range, _sort, sink| {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                sink.set_total(TOTAL);
                // Honest paging, capped window: never more than CAP rows back,
                // however wide the request.
                for idx in range.clone().take(CAP) {
                    if idx < TOTAL {
                        sink.push(idx, format!("id{idx}"), rec(&format!("v{idx}")))
                            .await?;
                    }
                }
                Ok(())
            }
        })
        .build()
        .map(Arc::new)
        .expect("build lens");

    let dio = lens.make_dio(master_cols(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().open().await?;

    scenery.set_viewport(0..CAP);
    settle().await;
    assert_eq!(
        scenery.row_count(),
        TOTAL,
        "a capped page is not the end of the set",
    );

    // Walk down it a window at a time; every row must eventually arrive. Each
    // step waits for its last row rather than for a fixed time — the rows are
    // the thing being asserted, and they announce themselves.
    for start in (0..TOTAL).step_by(CAP) {
        let end = (start + CAP).min(TOTAL);
        scenery.set_viewport(start..end);
        wait_for_row(&scenery, end - 1).await;
    }
    assert_eq!(scenery.row_count(), TOTAL, "the set did not shrink");
    assert!(
        scenery.row(TOTAL - 1).is_some(),
        "the last row never loaded — the clamp fired on an honest short page",
    );

    Ok(())
}
