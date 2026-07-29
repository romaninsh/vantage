//! A refresh must be able to see a set that GREW.
//!
//! A paged scenery holds a window, and a refresh re-fetches the contiguous
//! block it already has. That is right for rows that CHANGED — it overwrites
//! each slot in place without blanking the grid — but it cannot discover a row
//! appended past the end of that block: the refetch asks for exactly the range
//! already held, the source returns exactly that many rows, and a full page
//! carries no signal that the set is now longer.
//!
//! The user-visible shape (reported against a SQLite project): add a record,
//! the grid does not show it; press Refresh, still nothing; navigate away and
//! back and it appears — because reopening fetches `0..page_size` rather than
//! the block that was loaded.
//!
//! Sources that state a total in the response dodge this, since the refetch
//! learns the new total even when the rows it asked for are unchanged. SQL
//! drivers generally state nothing, which is why this surfaced there.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{Lens, TableScenery};
use vantage_types::Record;

mod support;
use support::chunk::master as master_cols;

/// Bounds a hang; not a speed assertion.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

/// A paged lens with **no `total_provider`** and a source that states no total
/// — the SQL shape. The only thing that can reveal a longer set is a fetch that
/// reaches past what is held.
fn paged_lens(cache: std::path::PathBuf, backend: Backend, seen: Arc<Mutex<Vec<Range<usize>>>>) -> Arc<Lens> {
    Lens::new()
        .cache_at(cache)
        .on_load_chunk(move |_dio, range, _sort, sink| {
            let backend = backend.clone();
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(range.clone());
                let rows = backend.lock().unwrap().clone();
                for idx in range {
                    if let Some((id, r)) = rows.get(idx) {
                        sink.push(idx, id.clone(), r.clone()).await?;
                    }
                }
                Ok(())
            }
        })
        .build()
        .map(Arc::new)
        .expect("build lens")
}

async fn wait_for_count(scenery: &Arc<dyn TableScenery>, want: usize) -> bool {
    let mut generations = scenery.subscribe();
    let reached = async {
        while scenery.row_count() != want {
            generations.changed().await.expect("generation watch closed");
        }
    };
    tokio::time::timeout(TIMEOUT, reached).await.is_ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn refresh_discovers_a_row_appended_past_the_loaded_window() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let backend: Backend = Arc::new(Mutex::new(
        (0..8)
            .map(|i| (format!("id{i}"), rec(&format!("v{i}"))))
            .collect(),
    ));
    let seen: Arc<Mutex<Vec<Range<usize>>>> = Arc::new(Mutex::new(Vec::new()));
    let lens = paged_lens(tmp.path().join("c.redb"), backend.clone(), seen.clone());

    let dio = lens.make_dio(master_cols(&[("v", "String")])).await?;
    let scenery = dio.table_scenery().open().await?;

    // The grid shows the whole set, so its viewport is exactly the rows that
    // exist — the case where "refetch what is loaded" and "refetch everything"
    // look identical, right up until the set grows.
    scenery.set_viewport(0..8);
    assert!(
        wait_for_count(&scenery, 8).await,
        "the initial window never loaded (got {})",
        scenery.row_count(),
    );

    // Someone adds a record. It lands in the source, past the loaded window.
    backend
        .lock()
        .unwrap()
        .push(("id8".to_string(), rec("v8")));

    // The Refresh button: `request_refresh` → `dio.refresh()` → `DatasetChanged`.
    dio.refresh().await?;

    assert!(
        wait_for_count(&scenery, 9).await,
        "refresh did not discover the appended row — still {} rows, ranges asked for: {:?}",
        scenery.row_count(),
        seen.lock().unwrap(),
    );
    assert!(
        scenery.row(8).is_some(),
        "row 8 is counted but was never fetched",
    );
    Ok(())
}

/// The reported shape: the source orders server-side, and the added row sorts
/// FIRST. Re-fetching only the loaded block then returns a shifted window — the
/// new row lands in slot 0 and the row that used to be last is displaced off
/// the end, so the grid shows a replacement instead of an addition.
#[tokio::test(flavor = "multi_thread")]
async fn an_inserted_row_that_sorts_first_does_not_displace_the_last_one() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    // Kept sorted by name on every read, the way `ORDER BY name` behaves.
    let backend: Backend = Arc::new(Mutex::new(vec![
        ("id_s".to_string(), rec("Sprocket")),
        ("id_f".to_string(), rec("Flange")),
        ("id_g".to_string(), rec("Grommet")),
    ]));
    let seen: Arc<Mutex<Vec<Range<usize>>>> = Arc::new(Mutex::new(Vec::new()));

    let sorted = backend.clone();
    let recorder = seen.clone();
    let lens = Lens::new()
        .cache_at(tmp.path().join("c.redb"))
        .on_load_chunk(move |_dio, range, _sort, sink| {
            let sorted = sorted.clone();
            let recorder = recorder.clone();
            async move {
                recorder.lock().unwrap().push(range.clone());
                let mut rows = sorted.lock().unwrap().clone();
                rows.sort_by(|a, b| {
                    let key = |r: &(String, Record<CborValue>)| match r.1.get("v") {
                        Some(CborValue::Text(s)) => s.clone(),
                        _ => String::new(),
                    };
                    key(a).cmp(&key(b))
                });
                for idx in range {
                    if let Some((id, r)) = rows.get(idx) {
                        sink.push(idx, id.clone(), r.clone()).await?;
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
    scenery.set_viewport(0..3);
    assert!(wait_for_count(&scenery, 3).await, "initial window never loaded");

    // "Cucumber" sorts ahead of Flange, Grommet and Sprocket.
    backend
        .lock()
        .unwrap()
        .push(("id_c".to_string(), rec("Cucumber")));
    dio.refresh().await?;

    assert!(
        wait_for_count(&scenery, 4).await,
        "the set grew to 4 but the view still holds {}",
        scenery.row_count(),
    );

    // Nothing may have been lost: all four names are present exactly once.
    let mut shown: Vec<String> = (0..scenery.row_count())
        .filter_map(|i| scenery.row(i))
        .filter_map(|r| match r.record.get("v") {
            Some(CborValue::Text(s)) => Some(s.clone()),
            _ => None,
        })
        .collect();
    shown.sort();
    assert_eq!(
        shown,
        vec!["Cucumber", "Flange", "Grommet", "Sprocket"],
        "a row was displaced rather than added; ranges asked for: {:?}",
        seen.lock().unwrap(),
    );
    Ok(())
}
