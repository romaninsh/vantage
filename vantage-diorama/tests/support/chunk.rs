//! Shared helpers for the chunk-loaded (paged/lazy) scenery tests.
//!
//! These integration tests (`chunk_sort`, `chunk_sort_fidelity`,
//! `chunk_refresh_reorder`, `chunk_warm_cache_order`, `chunk_refresh_no_transient`,
//! `refresh`) all drive a non-orderable paged master: metadata-only `Vista`, rows
//! served from an in-memory `Backend` by a paged loader, and a generation-watch
//! settle loop. The pieces that were copied verbatim across them live here.

#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_diorama::{Cache, Generation, Lens, Loader, TableScenery};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, Vista, VistaCapabilities, VistaMetadata, mocks::MockShell,
};

/// An in-memory list of `(id, record)` rows the lens serves chunks from.
pub type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

/// Master serving only metadata and `Window`, without `Order`; rows come from
/// `backend` through the paged loader. `cols` is the non-id column set (an `id`
/// String column flagged as the id is always added — see
/// [`master_with_id_type`] for a numeric one).
pub fn master(cols: &[(&str, &str)]) -> Vista {
    master_with_id_type("String", cols)
}

/// [`master`] with a declared type for the id column — `"Int"` for a source
/// whose rows carry a numeric primary key, which is a different shape for
/// everything that reads an id back out of a record.
pub fn master_with_id_type(id_ty: &str, cols: &[(&str, &str)]) -> Vista {
    master_as(id_ty, cols, VistaCapabilities::new())
}

/// [`master`] that also advertises `Search`: a view's search is pushed to
/// the paged loader rather than left over.
pub fn searchable_master(cols: &[(&str, &str)]) -> Vista {
    master_as(
        "String",
        cols,
        VistaCapabilities::new().with(Capability::Search),
    )
}

fn master_as(id_ty: &str, cols: &[(&str, &str)], capabilities: VistaCapabilities) -> Vista {
    let mut metadata = VistaMetadata::new().with_column(Column::new("id", id_ty).with_flag("id"));
    for (name, ty) in cols {
        metadata = metadata.with_column(Column::new(*name, *ty));
    }
    let metadata = metadata.with_id_column("id");
    // A native-order paged master: it serves windows through the paged loader
    // and genuinely can't push a sort down, so it advertises no `Order`. A
    // sorted view copies the whole backend through the loader's eager start
    // and sorts it locally.
    Vista::new(
        "items",
        MockShell::new()
            .with_metadata(metadata)
            .with_capabilities(capabilities.with(Capability::Window)),
    )
}

/// How a [`backend_loader`] serves the backend's rows.
#[derive(Clone, Copy, Default)]
struct Serving {
    /// Serve in descending order of this integer column instead of insertion
    /// order.
    native_desc: Option<&'static str>,
}

/// A paged loader serving `backend`: each row at its absolute index in the
/// served order, with the live `backend` length reported as the total, so a
/// refetch picks up appended rows. With `fail_next` set, the next load fails
/// once. Its eager start copies the whole backend, for a view whose order
/// the master can't take.
fn backend_loader(backend: Backend, serving: Serving, fail_next: Arc<AtomicBool>) -> Loader {
    let whole = backend.clone();
    let paged = Loader::paged(move |_cx, range, _query, sink| {
        let backend = backend.clone();
        let fail_next = fail_next.clone();
        async move {
            if fail_next.swap(false, Ordering::SeqCst) {
                return Err(vantage_core::error!("simulated chunk failure"));
            }
            let mut rows = backend.lock().unwrap().clone();
            if let Some(key) = serving.native_desc {
                rows.sort_by(|a, b| match (a.1.get(key), b.1.get(key)) {
                    (Some(CborValue::Integer(x)), Some(CborValue::Integer(y))) => {
                        i128::from(*y).cmp(&i128::from(*x))
                    }
                    _ => std::cmp::Ordering::Equal,
                });
            }
            sink.set_total(rows.len());
            for idx in range {
                if let Some((id, r)) = rows.get(idx) {
                    sink.push(idx, id.clone(), r.clone()).await?;
                }
            }
            Ok(())
        }
    });
    paged.or_eager(move |cx| {
        let rows: indexmap::IndexMap<_, _> = whole.lock().unwrap().iter().cloned().collect();
        async move { cx.reconcile(rows).await }
    })
}

/// A paged loader serving `backend` in insertion order, the live length
/// stated as the total, with an eager start copying the whole backend.
pub fn serve_backend(backend: Backend) -> Loader {
    backend_loader(backend, Serving::default(), Arc::default())
}

/// A Lens over a redb cache at `cache` with `loader`.
pub fn redb_lens(cache: std::path::PathBuf, loader: Loader) -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::redb(cache))
        .loader(loader)
        .build()
        .expect("build paged lens")
}

/// Paged lens with no refresher: refresh flows through the scenery's in-place
/// viewport refetch. Rows are served at their absolute `backend` index (the
/// master's native order).
pub fn paged_lens(cache: std::path::PathBuf, backend: Backend) -> Arc<Lens> {
    redb_lens(cache, serve_backend(backend))
}

/// [`paged_lens`] whose next chunk load fails once `fail_next` is set — a
/// transient source error (a 504) mid-refresh.
pub fn paged_lens_failing(
    cache: std::path::PathBuf,
    backend: Backend,
    fail_next: Arc<AtomicBool>,
) -> Arc<Lens> {
    redb_lens(
        cache,
        backend_loader(backend, Serving::default(), fail_next),
    )
}

/// Like [`paged_lens`] but rows are served in **native order** = integer
/// column `key` descending (mirrors the launches URL
/// `?ordering=-last_updated` / a `pos`-baked server order), windowed. This is
/// what a non-orderable paged master does: the client sort never reaches it.
pub fn paged_lens_native_desc(
    cache: std::path::PathBuf,
    backend: Backend,
    key: &'static str,
) -> Arc<Lens> {
    let serving = Serving {
        native_desc: Some(key),
    };
    redb_lens(cache, backend_loader(backend, serving, Arc::default()))
}

/// Wait until the generation watch advances past `current`, returning the new
/// generation. Panics on timeout / closed channel.
///
/// The timeout bounds a *hang*; it is not an assertion about speed. It used to
/// be 500ms, which is a speed assertion whether or not it was meant as one —
/// and one that fails on a busy CI runner compiling and testing the rest of the
/// workspace alongside it. A generation bump that takes a second is not a bug
/// this suite is trying to catch, so waiting longer costs nothing except on a
/// genuine failure, where the test still fails rather than hanging forever.
pub async fn wait_for_gen(rx: &mut tokio::sync::watch::Receiver<Generation>, current: u64) -> u64 {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if u64::from(*rx.borrow_and_update()) > current {
                return u64::from(*rx.borrow());
            }
            rx.changed().await.expect("watch channel closed");
        }
    })
    .await
    .expect("timed out waiting for generation bump")
}

/// Let the viewport debounce + chunk load settle.
///
/// The default viewport debounce is 50ms (`Tuning::default`), so the old 80ms
/// left 30ms for the load that follows it — thin enough that a scheduling hiccup
/// reads as a missing row. There are 14 call sites, so the extra wait costs a
/// couple of seconds across the suite and buys the same class of headroom as
/// [`wait_for_gen`]'s timeout.
pub async fn settle() {
    tokio::time::sleep(Duration::from_millis(250)).await;
}

/// Read a text column at a row index (None if absent / not a string).
pub fn col_at(scenery: &Arc<dyn TableScenery>, idx: usize, col: &str) -> Option<String> {
    scenery.row(idx).and_then(|r| match r.record.get(col) {
        Some(CborValue::Text(s)) => Some(s.clone()),
        _ => None,
    })
}

/// Collect a text column down every row, in row order.
pub fn order(scenery: &Arc<dyn TableScenery>, col: &str) -> Vec<String> {
    (0..scenery.row_count())
        .filter_map(|i| col_at(scenery, i, col))
        .collect()
}
