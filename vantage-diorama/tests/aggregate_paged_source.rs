//! Aggregates over a source that loads a page at a time.
//!
//! An eager source has all its rows before anything is derived from it, so the
//! first computation is also the right one. A paged source does not: a
//! dashboard opens against a cold cache, derives three figures over zero rows,
//! and the rows arrive seconds later.
//!
//! Nothing announces those rows per-record. A chunk load writes them straight
//! into the cache through `ChunkSink::push` and says only `RangeLoaded` — so an
//! aggregate that ignores that event computes once over the empty set and stays
//! there, showing a confident zero for as long as the page is open. This is
//! also what "progressive" means here: every page that lands recomputes over
//! everything held so far.

mod support;

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::aggregate::{Conditions, Count, Where};
use vantage_diorama::{Cache, Dio, Lens, Loader};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell};

use support::aggregate::{as_i64, record, settle, text, tuning};

/// Rows the paged source will serve, alternating status so a filtered count has
/// something to disagree with.
fn rows(n: usize) -> Vec<(String, Record<CborValue>)> {
    (0..n)
        .map(|i| {
            let status = if i % 2 == 0 { "open" } else { "done" };
            (format!("id{i}"), record(&[("status", text(status))]))
        })
        .collect()
}

/// A source that holds nothing until a viewport asks for it.
async fn paged_source(backing: Vec<(String, Record<CborValue>)>) -> Dio {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("status", "String"))
        .with_id_column("id");
    let vista = Vista::new(
        "items",
        MockShell::new()
            .with_metadata(metadata)
            .with_capabilities(VistaCapabilities::new()),
    );
    let lens = Lens::builder()
        .cache(Cache::memory())
        .tuning(tuning())
        .loader(Loader::paged(move |_cx, range, _query, sink| {
            let backing = backing.clone();
            async move {
                for idx in range {
                    if let Some((id, r)) = backing.get(idx) {
                        sink.push(idx, id.clone(), r.clone()).await?;
                    }
                }
                Ok(())
            }
        }))
        .build()
        .expect("paged lens builds");
    lens.dio(vista).build().await.expect("paged source dio")
}

/// The figures track rows as they arrive, page by page — they do not freeze at
/// whatever the cache held when they were derived.
#[tokio::test(flavor = "multi_thread")]
async fn aggregates_follow_a_paged_load() -> Result<()> {
    let src = paged_source(rows(10)).await;

    // Derived against an empty cache, exactly as a dashboard opens.
    let all = src.value_scenery(Count::rows()).open().await?;
    let open = src
        .value_scenery(Where::new(
            Conditions::new().eq("status", text("open")),
            Count::rows(),
        ))
        .open()
        .await?;
    settle().await;
    assert_eq!(as_i64(all.value()), Some(0), "nothing loaded yet");

    // First page lands.
    let scenery = src.table_scenery().open().await?;
    scenery.set_viewport(0..4, 0..0);
    wait_for(&all, 4).await;
    assert_eq!(as_i64(all.value()), Some(4), "the first page is counted");
    assert_eq!(as_i64(open.value()), Some(2), "and so is a filtered count");

    // Scrolling brings more; the figures grow with them.
    scenery.set_viewport(0..10, 0..0);
    wait_for(&all, 10).await;
    assert_eq!(as_i64(all.value()), Some(10));
    assert_eq!(as_i64(open.value()), Some(5));
    Ok(())
}

/// Wait for a derived value to reach `want`, bounding a hang without asserting
/// a speed: reaching it takes a chunk load plus the engine's debounce.
async fn wait_for(value: &Arc<dyn vantage_diorama::ValueScenery>, want: i64) {
    for _ in 0..60 {
        if as_i64(value.value()) == Some(want) {
            return;
        }
        settle().await;
    }
    panic!(
        "derived value settled at {:?}, wanted {want} — rows loaded by a chunk \
         fetch are not reaching the aggregation",
        as_i64(value.value()),
    );
}
