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
use vantage_diorama::{Dio, Lens};
use vantage_diorama_aggregate::{AggregateLens, Count, CountWhere};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell};

use support::{DEBOUNCE, record, settle, text};

fn lens() -> Arc<AggregateLens> {
    AggregateLens::in_memory()
        .debounce(DEBOUNCE)
        .build()
        .expect("aggregate lens builds")
}

fn as_i64(value: Option<CborValue>) -> Option<i64> {
    match value {
        Some(CborValue::Integer(i)) => Some(i128::from(i) as i64),
        _ => None,
    }
}

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
    let vista =
        Vista::new(
            "items",
            Box::new(MockShell::new().with_metadata(metadata).with_capabilities(
                VistaCapabilities {
                    can_order: false,
                    ..Default::default()
                },
            )),
        );
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_load_chunk(move |_dio, range, _sort, sink| {
                let backing = backing.clone();
                async move {
                    for idx in range {
                        if let Some((id, r)) = backing.get(idx) {
                            sink.push(idx, id.clone(), r.clone()).await?;
                        }
                    }
                    Ok(())
                }
            })
            .build()
            .expect("paged lens builds"),
    );
    lens.make_dio(vista).await.expect("paged source dio")
}

/// The figures track rows as they arrive, page by page — they do not freeze at
/// whatever the cache held when they were derived.
#[tokio::test(flavor = "multi_thread")]
async fn aggregates_follow_a_paged_load() -> Result<()> {
    let src = paged_source(rows(10)).await;
    let agg = lens();

    // Derived against an empty cache, exactly as a dashboard opens.
    let all = agg.value(&src, "all", Count::rows()).await?;
    let open = agg
        .value(&src, "open", CountWhere::new().eq("status", text("open")))
        .await?;
    settle().await;
    assert_eq!(as_i64(all.value()), Some(0), "nothing loaded yet");

    // First page lands.
    let scenery = src.table_scenery().open().await?;
    scenery.set_viewport(0..4);
    wait_for(&all, 4).await;
    assert_eq!(as_i64(all.value()), Some(4), "the first page is counted");
    assert_eq!(as_i64(open.value()), Some(2), "and so is a filtered count");

    // Scrolling brings more; the figures grow with them.
    scenery.set_viewport(0..10);
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
