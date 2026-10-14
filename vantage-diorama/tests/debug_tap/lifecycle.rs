//! The tap switch, the census lines, a paged load's correlated lifecycle,
//! and the silence of a lens built without the tap.

use vantage_diorama::LoadState;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

use crate::support::capture::{capture, lines_containing};
use crate::support::chunk::{master as chunk_master, serve_backend};
use crate::{hundred_rows, tap_lens, wait_until};

/// Minimal master Vista — an `id` column only, no rows. These tests only
/// exercise the tap plumbing, not data flow.
fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_id_column("id");
    Vista::new("books", MockShell::new().with_metadata(metadata))
}

#[tokio::test]
async fn builder_flag_reaches_the_dio_and_off_means_off() {
    let dio = tap_lens(true, None).dio(master()).build().await.unwrap();
    assert!(dio.debug_tap().enabled());
    assert_eq!(dio.debug_tap().ds(), "faker-ds");

    let dio = tap_lens(false, None).dio(master()).build().await.unwrap();
    assert!(!dio.debug_tap().enabled());
}

#[tokio::test]
async fn census_lines_fire_on_scenery_open_and_drop() {
    let (_guard, log) = capture();
    let dio = tap_lens(true, None).dio(master()).build().await.unwrap();

    let scenery = dio.table_scenery().open().await.unwrap();
    let opens = lines_containing(&log, "+1 table scenery");
    assert_eq!(opens.len(), 1);
    assert!(opens[0].contains("census"), "line: {}", opens[0]);
    assert!(opens[0].contains("now 1 table"), "line: {}", opens[0]);
    assert!(opens[0].contains("rss"), "census carries process stats");

    drop(scenery);
    // Guard teardown is synchronous; the census drop line is emitted from Drop.
    let closes = lines_containing(&log, "-1 table scenery");
    assert_eq!(closes.len(), 1);
}

/// A 100-row paged master served by the paged loader, with the debug tap on.
/// Exercises the full load lifecycle: dispatch/return correlated by `req`,
/// the resulting `Loading -> Complete` state transition, the stated total's
/// provenance, and a second pass over the same viewport hitting cache instead
/// of re-dispatching.
#[tokio::test]
async fn load_lifecycle_is_correlated_and_cache_hits_are_logged() {
    let (_guard, log) = capture();

    let dio = tap_lens(true, Some(serve_backend(hundred_rows())))
        .dio(chunk_master(&[("v", "String")]))
        .build()
        .await
        .unwrap();
    let scenery = dio.table_scenery().open().await.unwrap();

    // The whole set in one viewport, so this first load covers rows 0..100
    // and — together with the fetch's own `set_total(100)` — settles the
    // scenery at `Complete`, not merely `Partial`.
    scenery.set_viewport(0..100, 0..0);
    wait_until("first load return", || {
        lines_containing(&log, "got").len() == 1
    })
    .await;

    let dispatch = lines_containing(&log, "asks for");
    let ret = lines_containing(&log, "got");
    assert_eq!(dispatch.len(), 1);
    assert_eq!(ret.len(), 1);
    // The same req id ties them together.
    let req = dispatch[0]
        .split("fetch #")
        .nth(1)
        .expect("dispatch names its request")
        .split_whitespace()
        .next()
        .expect("request id")
        .to_string();
    assert!(
        ret[0].contains(&format!("fetch #{req} got")),
        "the return must carry the same request id: {}",
        ret[0]
    );
    assert!(ret[0].contains("rows in"));

    // A state transition to Complete was logged.
    let states = lines_containing(&log, "→ complete");
    assert!(
        states.iter().any(|l| l.contains("→ complete")),
        "{states:?}"
    );

    // The stated total (from `sink.set_total`) is provenance "stated".
    let totals = lines_containing(&log, "total");
    assert!(
        totals
            .iter()
            .any(|l| l.contains("100 rows") && l.contains("stated by the source")),
        "{totals:?}"
    );

    // Second pass over a viewport already fully covered by the cache: cache
    // hit, no second dispatch.
    scenery.set_viewport(0..30, 0..0);
    wait_until("cache hit on second pass", || {
        lines_containing(&log, "served locally").len() == 1
    })
    .await;
    assert_eq!(lines_containing(&log, "asks for").len(), 1, "no re-fetch");
}

/// The off-path silence regression: a lens built with no `.debug_datasource`
/// call must produce zero `vantage_diorama::debug` events across the same
/// load lifecycle `load_lifecycle_is_correlated_and_cache_hits_are_logged`
/// exercises — dispatch, return, the `Loading -> Complete` transition, and a
/// second viewport pass that hits cache. There is no log line to wait on when
/// the tap is off (that is the point), so this drives the same viewports and
/// waits on the scenery's own `LoadState` reaching `Complete` — row
/// materialization the caller can observe without the debug stream.
#[tokio::test]
async fn a_non_debug_lens_emits_nothing_on_the_load_path() {
    let (_guard, log) = capture();

    let dio = tap_lens(false, Some(serve_backend(hundred_rows())))
        .dio(chunk_master(&[("v", "String")]))
        .build()
        .await
        .unwrap();
    assert!(!dio.debug_tap().enabled());
    let scenery = dio.table_scenery().open().await.unwrap();

    // The whole set in one viewport, exactly as the debug-on lifecycle test
    // drives it — settles the scenery at `Complete` once the load lands.
    scenery.set_viewport(0..100, 0..0);
    wait_until("first load reaches Complete", || {
        scenery.load_state() == LoadState::Complete
    })
    .await;

    // Second pass over a viewport already fully covered by the cache — a
    // cache hit, same as the debug-on test. A hit changes no observable
    // scenery state (already `Complete`), so there is nothing to poll for;
    // sleeping past the 1ms viewport debounce is enough to let it settle
    // before the final assertion.
    scenery.set_viewport(0..30, 0..0);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(scenery.load_state(), LoadState::Complete);

    assert!(
        log.lock().unwrap().is_empty(),
        "debug-off lens must emit nothing: {:?}",
        log.lock().unwrap()
    );
}
