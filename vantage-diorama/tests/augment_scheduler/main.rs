//! The central augment scheduler, observed from the outside: one flight per
//! row across every open view, round-robin fairness between views with
//! disjoint viewports, a configurable worker pool, and withdrawal of queued
//! work when a view closes.
//!
//! The detail source is a gated shell: every fetch logs its id (dispatch
//! order — deterministic with the default single worker) and then blocks on a
//! semaphore the test releases permit by permit, so a test can hold the
//! worker mid-fetch while it arranges the next scene.

mod fairness;
mod sharing;
#[path = "../support/mod.rs"]
mod support;
mod workers;

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use support::{augmented_dio, fixed_augmentation, meta, record, size_detail};
use tempfile::TempDir;
use vantage_diorama::{Cache, CacheBackend, Dio, Lens, RedbCache, TableScenery, Tuning};
use vantage_types::Record;
use vantage_vista::Vista;
use vantage_vista::mocks::MockShell;

/// Master with `count` rows `r0..rN`, cheap columns only (id, modified).
fn master_vista(count: usize) -> Vista {
    let mut shell = MockShell::new();
    for i in 0..count {
        let id = format!("r{i}");
        shell = shell.with_record(&id, record(&[("id", &id), ("modified", "t1")]));
    }
    Vista::new("runs", shell.with_metadata(meta(&["id", "modified"])))
}

struct Fixture {
    dio: Dio,
    /// The detail source: its `get_log` and `max_concurrent_gets` observe
    /// the scheduler.
    detail: MockShell,
    gate: Arc<tokio::sync::Semaphore>,
    /// The Dio's cache backend, to read rows without the facade's hydration.
    cache: Arc<RedbCache>,
    _tmp: TempDir,
}

impl Fixture {
    /// The ids fetched so far, in dispatch order.
    fn log(&self) -> Vec<String> {
        self.detail.get_log()
    }

    /// The cached row `id`, as stored.
    async fn cached(&self, id: &str) -> Option<Record<CborValue>> {
        let table = self.cache.open_table("runs").await.unwrap();
        table.get_value(id).await.unwrap()
    }
}

/// A Dio over `rows` master rows, augmented with a gated `size` detail.
async fn fixture(rows: usize, workers: usize) -> Fixture {
    let tmp = TempDir::new().unwrap();
    let ids: Vec<String> = (0..rows).map(|i| format!("r{i}")).collect();
    let sizes: Vec<String> = (0..rows).map(|i| format!("{}", (i + 1) * 100)).collect();
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let detail = size_detail(
        ids.iter()
            .map(String::as_str)
            .zip(sizes.iter().map(String::as_str)),
    )
    .gated(gate.clone());

    let cache = Arc::new(RedbCache::open(tmp.path().join("cache.redb")).expect("redb opens"));
    let lens = Lens::builder()
        .cache(Cache::backend(cache.clone()))
        .tuning(
            Tuning::default()
                .with_viewport_debounce(Duration::from_millis(1))
                .with_augment_workers(workers),
        )
        .build()
        .expect("lens builds");
    let dio = augmented_dio(
        &lens,
        master_vista(rows),
        vec![fixed_augmentation(
            Vista::new("sizes", detail.clone()),
            &["size"],
        )],
    )
    .await;
    Fixture {
        dio,
        detail,
        gate,
        cache,
        _tmp: tmp,
    }
}

fn col_of(s: &Arc<dyn TableScenery>, i: usize, c: &str) -> Option<String> {
    s.row(i).and_then(|r| match r.record.get(c) {
        Some(CborValue::Text(t)) => Some(t.clone()),
        _ => None,
    })
}

async fn eventually(label: &str, f: impl Fn() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition '{label}' not met within timeout");
}
