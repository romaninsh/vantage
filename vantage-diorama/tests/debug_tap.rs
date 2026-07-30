//! Integration tests for the official debug stream (`vantage_diorama::debug`).

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

use ciborium::Value as CborValue;
use vantage_diorama::Lens;
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

mod support;
use support::chunk::{Backend, master as chunk_master};

/// Collects every `vantage_diorama::debug` event as one flat string:
/// `"<message> ds=<..> <field>=<..> ..."` — order of fields as emitted.
struct CaptureLayer(Arc<Mutex<Vec<String>>>);

struct FlatVisitor {
    message: String,
    fields: String,
}

impl Visit for FlatVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != "vantage_diorama::debug" {
            return;
        }
        let mut v = FlatVisitor {
            message: String::new(),
            fields: String::new(),
        };
        event.record(&mut v);
        self.0
            .lock()
            .unwrap()
            .push(format!("{}{}", v.message, v.fields));
    }
}

/// Install a thread-local subscriber that captures every
/// `vantage_diorama::debug` event as a flat string. Returns the guard
/// (drop it to uninstall) and the shared log the events land in.
///
/// Unused by this task's own test (which only checks the tap reaches the
/// Dio, off means off) — later tasks in this stream add events and use
/// this to assert on the emitted lines.
#[allow(dead_code)]
pub fn capture() -> (tracing::subscriber::DefaultGuard, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(log.clone()));
    (tracing::subscriber::set_default(subscriber), log)
}

/// Captured lines whose flattened text contains `needle`.
#[allow(dead_code)]
pub fn lines_containing(log: &Arc<Mutex<Vec<String>>>, needle: &str) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains(needle))
        .cloned()
        .collect()
}

/// Minimal master Vista — an `id` column only, no rows. These tests only
/// exercise the tap plumbing, not data flow.
fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    Vista::new("books", Box::new(MockShell::new().with_metadata(metadata)))
}

#[tokio::test]
async fn builder_flag_reaches_the_dio_and_off_means_off() {
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .debug_datasource("faker-ds")
            .runtime(tokio::runtime::Handle::current())
            .build()
            .unwrap(),
    );
    let dio = lens.make_dio(master()).await.unwrap();
    assert!(dio.debug_tap().enabled());
    assert_eq!(dio.debug_tap().ds(), "faker-ds");

    let quiet = Arc::new(
        Lens::new()
            .cache_in_memory()
            .runtime(tokio::runtime::Handle::current())
            .build()
            .unwrap(),
    );
    let dio = quiet.make_dio(master()).await.unwrap();
    assert!(!dio.debug_tap().enabled());
}

#[tokio::test]
async fn census_lines_fire_on_scenery_open_and_drop() {
    let (_guard, log) = capture();
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .debug_datasource("faker-ds")
            .runtime(tokio::runtime::Handle::current())
            .build()
            .unwrap(),
    );
    let dio = lens.make_dio(master()).await.unwrap();

    let scenery = dio.table_scenery().open().await.unwrap();
    let opens = lines_containing(&log, "census: table scenery opened");
    assert_eq!(opens.len(), 1);
    assert!(opens[0].contains("dio=\"books\""), "line: {}", opens[0]);
    assert!(opens[0].contains("table_sceneries=1"), "line: {}", opens[0]);
    assert!(opens[0].contains("uptime_ms="), "census carries process stats");

    drop(scenery);
    // Guard teardown is synchronous; the census drop line is emitted from Drop.
    let closes = lines_containing(&log, "census: table scenery closed");
    assert_eq!(closes.len(), 1);
}

/// Poll `pred` until it holds, with a bounded timeout — mirrors the
/// wait-for-condition pattern `tests/support/chunk.rs` uses around generation
/// watches, but polling the captured log instead (there is no single
/// generation bump to wait on: the second viewport pass settles on a "cache
/// hit" line, not a load).
async fn wait_until(label: &str, mut pred: impl FnMut() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while !pred() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for: {label}"));
}

/// A 100-row paged master served by `on_load_chunk`, with the debug tap on.
/// Exercises the full load lifecycle: dispatch/return correlated by `req`,
/// the resulting `Loading -> Complete` state transition, the stated total's
/// provenance, and a second pass over the same viewport hitting cache instead
/// of re-dispatching.
#[tokio::test]
async fn load_lifecycle_is_correlated_and_cache_hits_are_logged() {
    let (_guard, log) = capture();

    let backend: Backend = Arc::new(Mutex::new(
        (0..100)
            .map(|i| {
                let mut r = Record::new();
                r.insert("v".to_string(), CborValue::Text(format!("row{i}")));
                (format!("id{i}"), r)
            })
            .collect(),
    ));

    let lens = {
        let backend = backend.clone();
        Arc::new(
            Lens::new()
                .cache_in_memory()
                .debug_datasource("faker-ds")
                .viewport_debounce(std::time::Duration::from_millis(1))
                .runtime(tokio::runtime::Handle::current())
                .on_load_chunk(move |_dio, range, _query, sink| {
                    let backend = backend.clone();
                    async move {
                        let rows = backend.lock().unwrap().clone();
                        sink.set_total(rows.len());
                        for idx in range {
                            if let Some((id, r)) = rows.get(idx) {
                                sink.push(idx, id.clone(), r.clone()).await?;
                            }
                        }
                        Ok(())
                    }
                })
                .build()
                .unwrap(),
        )
    };

    let dio = lens
        .make_dio(chunk_master(&[("v", "String")]))
        .await
        .unwrap();
    let scenery = dio.table_scenery().open().await.unwrap();

    // The whole set in one viewport, so this first load covers rows 0..100
    // and — together with the fetch's own `set_total(100)` — settles the
    // scenery at `Complete`, not merely `Partial`.
    scenery.set_viewport(0..100);
    wait_until("first load return", || {
        lines_containing(&log, "load return").len() == 1
    })
    .await;

    let dispatch = lines_containing(&log, "load dispatch");
    let ret = lines_containing(&log, "load return");
    assert_eq!(dispatch.len(), 1);
    assert_eq!(ret.len(), 1);
    // The same req id ties them together.
    let req = dispatch[0]
        .split("req=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    assert!(ret[0].contains(&format!("req={req}")));
    assert!(ret[0].contains("ms="));

    // A state transition to Complete was logged.
    let states = lines_containing(&log, "state");
    assert!(
        states.iter().any(|l| l.contains("to=\"Complete\"")),
        "{states:?}"
    );

    // The stated total (from `sink.set_total`) is provenance "stated".
    let totals = lines_containing(&log, "total");
    assert!(
        totals
            .iter()
            .any(|l| l.contains("total=100") && l.contains("provenance=\"stated\"")),
        "{totals:?}"
    );

    // Second pass over a viewport already fully covered by the cache: cache
    // hit, no second dispatch.
    scenery.set_viewport(0..30);
    wait_until("cache hit on second pass", || {
        lines_containing(&log, "cache hit").len() == 1
    })
    .await;
    assert_eq!(
        lines_containing(&log, "load dispatch").len(),
        1,
        "no re-fetch"
    );
}

/// A single row with 50 fat extra fields — the wide-data case the "columns"
/// line exists to surface. The scenery is opened plainly (no `.columns()`
/// demand), so the Dio's demand union is `None` and the line reports
/// `demanded="all"`; `received_count`/`payload_bytes` are the interesting
/// signal here.
#[tokio::test]
async fn column_line_exposes_undemanded_wide_fields() {
    let (_guard, log) = capture();

    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .debug_datasource("faker-ds")
            .viewport_debounce(std::time::Duration::from_millis(1))
            .runtime(tokio::runtime::Handle::current())
            .on_load_chunk(move |_dio, range, _query, sink| async move {
                for idx in range {
                    if idx != 0 {
                        continue;
                    }
                    let mut r = Record::new();
                    r.insert("id".to_string(), CborValue::Text("row0".to_string()));
                    r.insert("name".to_string(), CborValue::Text("Row Zero".to_string()));
                    for n in 1..=50 {
                        r.insert(
                            format!("extra_{n:04}"),
                            CborValue::Text("x".repeat(1024)),
                        );
                    }
                    sink.set_total(1);
                    sink.push(idx, "row0".to_string(), r).await?;
                }
                Ok(())
            })
            .build()
            .unwrap(),
    );

    let dio = lens
        .make_dio(chunk_master(&[("name", "String")]))
        .await
        .unwrap();
    let scenery = dio.table_scenery().open().await.unwrap();

    scenery.set_viewport(0..1);
    wait_until("first load return", || {
        lines_containing(&log, "load return").len() == 1
    })
    .await;

    let cols = lines_containing(&log, "columns");
    assert_eq!(cols.len(), 1, "{cols:?}");
    assert!(cols[0].contains("demanded=\"all\""), "{}", cols[0]);
    assert!(cols[0].contains("received_count=52"), "{}", cols[0]);
    assert!(cols[0].contains("payload_bytes="), "{}", cols[0]);
    // payload should be dominated by the extras: > 50KB for 1 row wouldn't
    // hold for all rows; just assert it's large.
    let bytes: usize = cols[0]
        .split("payload_bytes=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert!(bytes > 50_000, "wide payload must be visible: {bytes}");
}

/// A chunk load's cache write reports how many of the written rows were new
/// vs. already-cached updates, and the resulting percent-cached against the
/// known total.
#[tokio::test]
async fn cache_writes_report_new_updated_and_percentage() {
    let (_guard, log) = capture();

    let backend: Backend = Arc::new(Mutex::new(
        (0..100)
            .map(|i| {
                let mut r = Record::new();
                r.insert("v".to_string(), CborValue::Text(format!("row{i}")));
                (format!("id{i}"), r)
            })
            .collect(),
    ));

    let lens = {
        let backend = backend.clone();
        Arc::new(
            Lens::new()
                .cache_in_memory()
                .debug_datasource("faker-ds")
                .viewport_debounce(std::time::Duration::from_millis(1))
                .runtime(tokio::runtime::Handle::current())
                .on_load_chunk(move |_dio, range, _query, sink| {
                    let backend = backend.clone();
                    async move {
                        let rows = backend.lock().unwrap().clone();
                        sink.set_total(rows.len());
                        for idx in range {
                            if let Some((id, r)) = rows.get(idx) {
                                sink.push(idx, id.clone(), r.clone()).await?;
                            }
                        }
                        Ok(())
                    }
                })
                .build()
                .unwrap(),
        )
    };

    let dio = lens
        .make_dio(chunk_master(&[("v", "String")]))
        .await
        .unwrap();
    let scenery = dio.table_scenery().open().await.unwrap();

    scenery.set_viewport(0..30);
    wait_until("first load return", || {
        lines_containing(&log, "load return").len() == 1
    })
    .await;

    let writes = lines_containing(&log, "cache write");
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(writes[0].contains("new=30"), "{}", writes[0]);
    assert!(writes[0].contains("updated=0"), "{}", writes[0]);
    assert!(writes[0].contains("known_total=100"), "{}", writes[0]);
    // 30 of 100 rows → 30%.
    assert!(writes[0].contains("cached_pct=30"), "{}", writes[0]);
}
