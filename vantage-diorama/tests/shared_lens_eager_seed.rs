//! One lens serving both shapes: the eager seed must still reach open sceneries.
//!
//! vantage-ui builds one Lens per datasource, so that lens carries every
//! callback any table under it might need — `on_start` to copy a whole set,
//! `on_load_chunk` to window a pageable one. Which applies is a property of the
//! master, decided per scenery at open (`builder::pages_lazily`) and recorded
//! as `TableSceneryState::paged`.
//!
//! The shape that broke: an eager scenery opens over a cold cache, shows
//! nothing (correctly — there is nothing yet), and the detached `on_start`
//! lands its rows a moment later and announces `DatasetChanged`. The reactor
//! answers a whole-set event one of two ways, and it was choosing between them
//! by asking the *lens* whether an `on_load_chunk` existed. Under a shared lens
//! one always does, so an eager scenery took the paged branch: re-count, then
//! re-fetch the current viewport — through a callback the loader will not
//! dispatch for it. Nothing reseeded the visible map from the cache, so the
//! grid stayed empty for as long as it stayed open. Reopening it fixed it,
//! because `open()` seeds from what is by then a warm cache — which is exactly
//! how this presented: blank on first visit, instant on the second.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Lens, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

/// Bounds a hang; not an assertion about speed.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn named(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r
}

/// An eager master: it holds rows and, crucially, does **not** advertise
/// `can_fetch_window` — so a lens offering both shapes is warmed whole.
fn master(rows: &[(&str, &str)]) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    let mut shell = MockShell::new().with_metadata(metadata);
    for (id, name) in rows {
        shell = shell.with_record(*id, named(name));
    }
    Vista::new("items", Box::new(shell))
}

/// Block until the scenery reports `want` rows, driven by the generation watch.
async fn wait_for_rows(scenery: &Arc<dyn TableScenery>, want: usize) {
    let mut generations = scenery.subscribe();
    let reached = async {
        while scenery.row_count() != want {
            generations
                .changed()
                .await
                .expect("generation watch closed");
        }
    };
    if tokio::time::timeout(TIMEOUT, reached).await.is_err() {
        panic!(
            "scenery settled at {} rows (wanted {want}) — the eager seed never \
             reached the visible map",
            scenery.row_count(),
        );
    }
}

/// The detached seed reaches a scenery that opened before it landed, and the
/// paged callback the shared lens also carries is never dispatched for it.
#[tokio::test(flavor = "multi_thread")]
async fn eager_seed_reaches_a_scenery_opened_over_a_cold_cache() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    // Held shut until the scenery is open, so the seed is guaranteed to land
    // *after* it — the ordering the bug needs, which a race would otherwise
    // only sometimes produce.
    let gate = Arc::new(tokio::sync::Notify::new());
    let chunk_calls = Arc::new(AtomicUsize::new(0));

    let opener = gate.clone();
    let counted = chunk_calls.clone();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("c.redb"))
            .on_start_blocking(false)
            .on_start(move |dio| {
                let dio = dio.clone();
                let opener = opener.clone();
                async move {
                    opener.notified().await;
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            // The other shape the shared lens carries. An eager scenery must
            // never route through it: this master was never asked whether it
            // can serve a window, and in the app that mis-dispatch surfaced as
            // a "get_windowed not supported" toast.
            .on_load_chunk(move |_dio, _range, _sort, _sink| {
                let counted = counted.clone();
                async move {
                    counted.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    );

    let dio = lens
        .make_dio(master(&[("a", "Ann"), ("b", "Bob"), ("c", "Cid")]))
        .await?;
    let scenery = dio.table_scenery().open().await?;
    assert_eq!(
        scenery.row_count(),
        0,
        "nothing is cached yet — the seed is still gated",
    );

    // Release the seed. Its `DatasetChanged` is the only thing that can put
    // these rows on screen.
    gate.notify_one();
    wait_for_rows(&scenery, 3).await;

    assert_eq!(
        chunk_calls.load(Ordering::SeqCst),
        0,
        "an eager scenery fetched a window through the shared lens's pager",
    );
    Ok(())
}
