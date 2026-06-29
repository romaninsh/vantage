//! Shared helpers for the chunk-loaded (paged/lazy) scenery tests.
//!
//! These integration tests (`chunk_sort`, `chunk_sort_fidelity`,
//! `chunk_refresh_reorder`, `chunk_warm_cache_order`, `chunk_refresh_no_transient`)
//! all drive a non-orderable paged master: metadata-only `Vista`, rows served from
//! an in-memory `Backend` via `on_load_chunk`, and a generation-watch settle loop.
//! The pieces that were copied verbatim across them live here.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_diorama::{Generation, Lens, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

/// An in-memory list of `(id, record)` rows the lens serves chunks from.
pub type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

/// Master serving only metadata + the (false) order capability; rows come from
/// `backend` via `on_load_chunk`. `cols` is the non-id column set (an `id`
/// String column flagged as the id is always added).
pub fn master(cols: &[(&str, &str)]) -> Vista {
    let mut metadata =
        VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id"));
    for (name, ty) in cols {
        metadata = metadata.with_column(Column::new(*name, *ty));
    }
    let metadata = metadata.with_id_column("id");
    Vista::new("items", Box::new(MockShell::new().with_metadata(metadata)))
}

/// Paged lens with NO `on_refresh`: refresh flows through the scenery's in-place
/// viewport refetch. `on_load_chunk` pushes each row at its absolute `backend`
/// index (the master's native order). `total_provider` reports the live
/// `backend` length, so a re-count picks up appended rows.
pub fn paged_lens(cache: std::path::PathBuf, backend: Backend) -> Arc<Lens> {
    let total = backend.clone();
    let lens = Lens::new()
        .cache_at(cache)
        .total_provider(move |_dio| {
            let b = total.clone();
            async move { Ok(b.lock().unwrap().len()) }
        })
        .on_load_chunk(move |_dio, range, sink| {
            let b = backend.clone();
            async move {
                let rows = b.lock().unwrap().clone();
                for idx in range {
                    if let Some((id, r)) = rows.get(idx) {
                        sink.push(idx, id.clone(), r.clone()).await?;
                    }
                }
                Ok(())
            }
        })
        .build()
        .expect("build paged lens");
    Arc::new(lens)
}

/// Like [`paged_lens`] but `on_load_chunk` serves rows in **native order** =
/// integer column `key` descending (mirrors the launches URL
/// `?ordering=-last_updated` / a `pos`-baked server order), windowed. This is
/// what a non-orderable paged master does: the client sort never reaches it.
pub fn paged_lens_native_desc(
    cache: std::path::PathBuf,
    backend: Backend,
    key: &'static str,
) -> Arc<Lens> {
    let total = backend.clone();
    let lens = Lens::new()
        .cache_at(cache)
        .total_provider(move |_dio| {
            let b = total.clone();
            async move { Ok(b.lock().unwrap().len()) }
        })
        .on_load_chunk(move |_dio, range, sink| {
            let b = backend.clone();
            async move {
                let mut rows = b.lock().unwrap().clone();
                rows.sort_by(|a, b| {
                    let la = a.1.get(key);
                    let lb = b.1.get(key);
                    match (la, lb) {
                        (Some(CborValue::Integer(x)), Some(CborValue::Integer(y))) => {
                            i128::from(*y).cmp(&i128::from(*x))
                        }
                        _ => std::cmp::Ordering::Equal,
                    }
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
        .expect("build paged lens");
    Arc::new(lens)
}

/// Wait until the generation watch advances past `current`, returning the new
/// generation. Panics on timeout / closed channel.
pub async fn wait_for_gen(rx: &mut tokio::sync::watch::Receiver<Generation>, current: u64) -> u64 {
    tokio::time::timeout(Duration::from_millis(500), async {
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
pub async fn settle() {
    tokio::time::sleep(Duration::from_millis(80)).await;
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
