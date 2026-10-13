//! Backends, waits and the gated lens the loader-priority tests share.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::{Priority, Result};
use vantage_diorama::{ChunkSink, Dio, Lens, LensBuilder, Loader, TableScenery};
use vantage_types::Record;

use crate::support::chunk::master;

pub type Backend = Arc<Mutex<Vec<(String, Record<CborValue>)>>>;

pub fn rec(v: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("v".to_string(), CborValue::Text(v.to_string()));
    r
}

pub fn rows(n: usize) -> Backend {
    Arc::new(Mutex::new(
        (0..n)
            .map(|i| (format!("r{i}"), rec(&format!("v{i}"))))
            .collect(),
    ))
}

/// Poll `pred` every 5ms until it holds; panic naming `label` if two seconds
/// pass first. The budget bounds a *hang* — it is not an assertion about
/// speed, which is why it is generous — and the label is what turns a timeout
/// into a diagnosis rather than a stack trace in a loop.
pub async fn wait_until(label: &str, mut pred: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !pred() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out after 2s waiting for: {label}",
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Serve the rows of `range` that `snapshot` holds — the body every paged
/// loader in this binary shares. It takes a snapshot rather than the backend
/// so a gated loader reads the same rows either side of its gate.
pub async fn push_rows(
    snapshot: &[(String, Record<CborValue>)],
    range: impl IntoIterator<Item = usize>,
    sink: &ChunkSink,
) -> Result<()> {
    for idx in range {
        if let Some((id, r)) = snapshot.get(idx) {
            sink.push(idx, id.clone(), r.clone()).await?;
        }
    }
    Ok(())
}

/// A paged loader whose callback blocks on a gate until the test releases
/// it, and records what it ran as, which ranges it was asked for, and whether
/// its future was dropped before finishing. Every window states the
/// backend's length as the total.
pub struct Gated {
    pub release: Arc<tokio::sync::Notify>,
    pub calls: Arc<Mutex<Vec<std::ops::Range<usize>>>>,
    pub priorities: Arc<Mutex<Vec<Priority>>>,
    pub cancelled: Arc<AtomicUsize>,
    /// How many rows the callback pushes before it blocks. Enough by default
    /// that a test can change one of them behind the loader's back and watch
    /// what a cancellation does to it; zero when a test needs the in-flight
    /// load to leave the visible map strictly alone (an early-pushed row
    /// moves `next_load_more_start`, and one test depends on where that
    /// lands).
    pub early_push: usize,
}

impl Gated {
    pub fn new() -> Self {
        Self {
            release: Arc::new(tokio::sync::Notify::new()),
            calls: Arc::new(Mutex::new(Vec::new())),
            priorities: Arc::new(Mutex::new(Vec::new())),
            cancelled: Arc::new(AtomicUsize::new(0)),
            early_push: 4,
        }
    }

    pub fn priority_of_call(&self, n: usize) -> Option<Priority> {
        self.priorities.lock().unwrap().get(n).copied()
    }

    pub fn calls_len(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// The gated paged loader over `backend`.
    fn loader(&self, backend: Backend) -> Loader {
        let release = self.release.clone();
        let calls = self.calls.clone();
        let priorities = self.priorities.clone();
        let cancelled = self.cancelled.clone();
        let early_push = self.early_push;
        Loader::paged(move |_cx, range, _query, sink| {
            let b = backend.clone();
            let release = release.clone();
            let calls = calls.clone();
            let priorities = priorities.clone();
            let cancelled = cancelled.clone();
            async move {
                calls.lock().unwrap().push(range.clone());
                priorities.lock().unwrap().push(Priority::current());
                let mut flag = DropFlag(cancelled, false);
                let rows = b.lock().unwrap().clone();
                sink.set_total(rows.len());
                // Push the first few rows before blocking, so a test can
                // observe them bound in the visible map while the rest of
                // the range is still pending — and, if this call gets
                // cancelled, check what happened to them.
                let mut range = range;
                push_rows(&rows, range.by_ref().take(early_push), &sink).await?;
                release.notified().await;
                push_rows(&rows, range, &sink).await?;
                flag.1 = true; // finished: not a cancellation
                Ok(())
            }
        })
    }

    /// A Lens over a redb cache at `cache` with the gated loader.
    pub fn lens(&self, cache: std::path::PathBuf, backend: Backend) -> Arc<Lens> {
        self.lens_from(Lens::builder(), cache, backend)
    }

    /// [`lens`](Self::lens) on top of `builder`, for a test that needs another
    /// setting as well.
    pub fn lens_from(
        &self,
        builder: LensBuilder,
        cache: std::path::PathBuf,
        backend: Backend,
    ) -> Arc<Lens> {
        builder
            .cache(vantage_diorama::Cache::redb(cache))
            .loader(self.loader(backend))
            .build()
            .expect("lens")
    }
}

/// Counts a loader future dropped before it finished.
struct DropFlag(Arc<AtomicUsize>, bool);
impl Drop for DropFlag {
    fn drop(&mut self) {
        if !self.1 {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// A gated lens over `backend` in a fresh redb cache, a Dio on the plain `v`
/// master, and a 10-row scenery whose on-open fetch has been let through.
pub struct GatedScenery {
    _tmp: TempDir,
    _dio: Dio,
    pub scenery: Arc<dyn TableScenery>,
}

impl GatedScenery {
    pub async fn open(gated: &Gated, backend: Backend) -> Result<Self> {
        let tmp = TempDir::new().unwrap();
        let lens = gated.lens(tmp.path().join("c.redb"), backend);
        let dio = lens.dio(master(&[("v", "String")])).build().await?;
        let scenery = dio.table_scenery().page_size(10).open().await?;
        release_the_open_fetch(gated, &scenery).await;
        Ok(Self {
            _tmp: tmp,
            _dio: dio,
            scenery,
        })
    }
}

/// Cancel the in-flight load by scrolling to 20..30, then let that load
/// through and wait for its rows.
pub async fn supersede_with_20_30(gated: &Gated, scenery: &Arc<dyn TableScenery>) {
    scenery.set_viewport(20..30);
    wait_until("the superseding load to start", || gated.calls_len() >= 3).await;
    gated.release.notify_one();
    wait_until("row 25", || scenery.row(25).is_some()).await;
}

/// Let the on-open fetch (call 1) through and wait for a row only the
/// remainder loop writes — unlike an early-pushed row, row 5 cannot already
/// be present before the release is actually consumed, so waiting on it
/// proves call 1 finished rather than merely started.
pub async fn release_the_open_fetch(gated: &Gated, scenery: &Arc<dyn TableScenery>) {
    wait_until("the on-open fetch to start", || gated.calls_len() >= 1).await;
    gated.release.notify_one();
    wait_until("the on-open fetch to finish", || scenery.row(5).is_some()).await;
}
