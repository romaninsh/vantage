//! One lens serving both shapes: the eager seed must still reach open sceneries.
//!
//! vantage-ui builds one Lens per datasource, so that lens carries every
//! loader any table under it might need — `Loader::paged(..).or_eager(..)`:
//! an eager start to copy a whole set, a paged loader to window a pageable
//! one. Which applies is a property of the master, decided per scenery at open
//! (`builder::open::pages_lazily`) and recorded as `SharedRows::paged`.
//!
//! The shape that broke: an eager scenery opens over a cold cache, shows
//! nothing (correctly — there is nothing yet), and the detached eager start
//! lands its rows a moment later and announces `DatasetChanged`. The reactor
//! answers a whole-set event one of two ways, and it was choosing between them
//! by asking the *lens* whether a paged loader existed. Under a shared lens
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
use vantage_diorama::{Cache, Lens, Loader, TableScenery};
use vantage_types::Record;
use vantage_vista::{Capability, Column, TableShell, Vista, VistaMetadata, mocks::MockShell};

/// Bounds a hang; not an assertion about speed.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn named(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r
}

/// An eager master: it holds rows and, crucially, does **not** advertise
/// `Window` — so a lens offering both shapes is warmed whole.
fn master(rows: &[(&str, &str)]) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    let shell = MockShell::new();
    let capabilities = shell.capabilities().without(Capability::Window);
    let mut shell = shell
        .with_metadata(metadata)
        .with_capabilities(capabilities);
    for (id, name) in rows {
        shell = shell.with_record(*id, named(name));
    }
    Vista::new("items", shell)
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
    // The pager is the other shape the shared lens carries. An eager scenery
    // must never route through it: this master was never asked whether it can
    // serve a window, and in the app that mis-dispatch surfaced as a
    // "get_windowed not supported" toast.
    let loader = Loader::paged(move |_cx, _range, _query, _sink| {
        let counted = counted.clone();
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .or_eager(move |cx| {
        let opener = opener.clone();
        async move {
            opener.notified().await;
            let rows = cx.master().list_values().await?;
            cx.cache().insert_values(rows).await
        }
    })
    .blocking(false);
    let lens = Lens::builder()
        .cache(Cache::redb(tmp.path().join("c.redb")))
        .loader(loader)
        .build()
        .expect("build lens");

    let dio = lens
        .dio(master(&[("a", "Ann"), ("b", "Bob"), ("c", "Cid")]))
        .build()
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
