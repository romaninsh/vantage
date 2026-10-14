//! Integration tests for the official debug stream (`vantage_diorama::debug`).

mod lifecycle;
mod payload;
#[path = "../support/mod.rs"]
mod support;
mod two_pass;

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use vantage_diorama::{Cache, Lens, Loader, Tuning};
use vantage_types::Record;

use support::chunk::Backend;

/// A Lens over an in-memory cache with a 1ms viewport debounce, the debug
/// tap on (datasource `faker-ds`) when `debug`, and `loader` if given.
fn tap_lens(debug: bool, loader: Option<Loader>) -> Arc<Lens> {
    let mut builder = Lens::builder()
        .cache(Cache::memory())
        .tuning(Tuning::default().with_viewport_debounce(std::time::Duration::from_millis(1)))
        .runtime(tokio::runtime::Handle::current());
    if debug {
        builder = builder.debug_datasource("faker-ds");
    }
    if let Some(loader) = loader {
        builder = builder.loader(loader);
    }
    builder.build().unwrap()
}

/// A 100-row backend: ids `id0..id99`, each with a `v` column `row<i>`.
fn hundred_rows() -> Backend {
    Arc::new(Mutex::new(
        (0..100)
            .map(|i| {
                let mut r = Record::new();
                r.insert("v".to_string(), CborValue::Text(format!("row{i}")));
                (format!("id{i}"), r)
            })
            .collect(),
    ))
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
