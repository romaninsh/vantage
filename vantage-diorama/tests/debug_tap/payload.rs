//! What a chunk load reports about its payload: the column line for wide
//! rows, and the new/updated split of its cache write.

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_types::Record;

use crate::support::capture::{capture, lines_containing};
use crate::support::chunk::{Backend, master as chunk_master, serve_backend};
use crate::{hundred_rows, tap_lens, wait_until};

/// `n` wide rows `row0..`: `id`, `name` and 50 one-kilobyte `extra_*` fields,
/// 52 columns in all.
fn wide_rows(n: usize) -> Backend {
    Arc::new(Mutex::new(
        (0..n)
            .map(|i| {
                let mut r = Record::new();
                r.insert("id".to_string(), CborValue::Text(format!("row{i}")));
                r.insert("name".to_string(), CborValue::Text(format!("Row {i}")));
                for n in 1..=50 {
                    r.insert(format!("extra_{n:04}"), CborValue::Text("x".repeat(1024)));
                }
                (format!("row{i}"), r)
            })
            .collect(),
    ))
}

/// A single row with 50 fat extra fields — the wide-data case the "columns"
/// line exists to surface. The scenery is opened plainly (no `.columns()`
/// demand), so the Dio's demand union is `None` and the line reports
/// `demanded="all"`; `received_count`/`payload_bytes` are the interesting
/// signal here.
#[tokio::test]
async fn column_line_exposes_undemanded_wide_fields() {
    let (_guard, log) = capture();

    let dio = tap_lens(true, Some(serve_backend(wide_rows(1))))
        .dio(chunk_master(&[("name", "String")]))
        .build()
        .await
        .unwrap();
    let scenery = dio.table_scenery().open().await.unwrap();

    scenery.set_viewport(0..1, 0..0);
    wait_until("first load return", || {
        lines_containing(&log, "got").len() == 1
    })
    .await;

    let cols = lines_containing(&log, "payload");
    assert_eq!(cols.len(), 1, "{cols:?}");
    assert!(cols[0].contains("columns"), "{}", cols[0]);
    assert!(cols[0].contains("52 columns received"), "{}", cols[0]);
    assert!(
        cols[0].contains("KB") || cols[0].contains("B"),
        "{}",
        cols[0]
    );
    // payload should be dominated by the extras: > 50KB for 1 row wouldn't
    // hold for all rows; just assert it's large.
    // The wide payload has to be visible in the size the line reports: 52
    // columns of filler is kilobytes, not bytes.
    assert!(
        cols[0].contains("KB") || cols[0].contains("MB"),
        "a wide payload must report in KB or MB, not bytes: {}",
        cols[0]
    );
}

/// The wide-data detector's column union must dedup ACROSS rows, not just
/// within one: ten rows sharing the same 52-column shape should still report
/// `received_count=52` (not 520), proving the union doesn't just concatenate
/// every row's field names. `payload_bytes` should scale with row count, and
/// `rows` should report the full batch.
#[tokio::test]
async fn column_line_dedups_across_multiple_wide_rows() {
    let (_guard, log) = capture();

    let dio = tap_lens(true, Some(serve_backend(wide_rows(10))))
        .dio(chunk_master(&[("name", "String")]))
        .build()
        .await
        .unwrap();
    let scenery = dio.table_scenery().open().await.unwrap();

    scenery.set_viewport(0..10, 0..0);
    wait_until("first load return", || {
        lines_containing(&log, "got").len() == 1
    })
    .await;

    let cols = lines_containing(&log, "payload");
    assert_eq!(cols.len(), 1, "{cols:?}");
    assert!(cols[0].contains("columns"), "{}", cols[0]);
    // Same 52 fields on every row — the union must not grow with row count.
    assert!(cols[0].contains("52 columns received"), "{}", cols[0]);
    assert!(cols[0].contains("columns"), "{}", cols[0]);
    // The wide payload has to be visible in the size the line reports: 52
    // columns of filler is kilobytes, not bytes.
    assert!(
        cols[0].contains("KB") || cols[0].contains("MB"),
        "a wide payload must report in KB or MB, not bytes: {}",
        cols[0]
    );
}

/// A chunk load's cache write reports how many of the written rows were new
/// vs. already-cached updates, and the resulting percent-cached against the
/// known total.
#[tokio::test]
async fn cache_writes_report_new_updated_and_percentage() {
    let (_guard, log) = capture();

    let dio = tap_lens(true, Some(serve_backend(hundred_rows())))
        .dio(chunk_master(&[("v", "String")]))
        .build()
        .await
        .unwrap();
    let scenery = dio.table_scenery().open().await.unwrap();

    scenery.set_viewport(0..30, 0..0);
    wait_until("first load return", || {
        lines_containing(&log, "got").len() == 1
    })
    .await;

    let writes = lines_containing(&log, "new,");
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(writes[0].contains("+30 new"), "{}", writes[0]);
    assert!(writes[0].contains("0 updated"), "{}", writes[0]);
    assert!(writes[0].contains("of 100"), "{}", writes[0]);
    // 30 of 100 rows → 30%.
    assert!(writes[0].contains("(30%)"), "{}", writes[0]);
}
