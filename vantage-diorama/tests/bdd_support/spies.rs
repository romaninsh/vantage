use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct Spies {
    pub on_start: Arc<AtomicU64>,
    pub on_refresh: Arc<AtomicU64>,
    pub on_event: Arc<AtomicU64>,
    pub on_flash: Arc<AtomicU64>,
    pub total_provider: Arc<AtomicU64>,
    pub on_load_chunk: Arc<AtomicU64>,
    /// Bumped by the test `on_start` / `on_load_chunk` closures right
    /// before they call `master.list_values()` — gives the "row(i)
    /// doesn't fetch the master" scenarios a stable counter to assert
    /// against without wrapping MockShell.
    pub master_list_calls: Arc<AtomicU64>,
    /// Last range requested via `on_load_chunk`. Used by the
    /// "coalesce" scenario to assert the surviving load was for the
    /// most recent viewport.
    pub last_load_chunk_range: Arc<Mutex<Option<Range<usize>>>>,
    /// One-shot fault injector. When set true, the next `on_load_chunk`
    /// invocation returns `Err` and clears the flag. Drives the
    /// `LoadFailed` scenarios without rebuilding the lens.
    pub on_load_chunk_error_once: Arc<AtomicBool>,
    /// Virtual-time latency (ms) the scriptable source sleeps before each
    /// gated read (currently `on_load_chunk`). `0` = none. Lets a scenario
    /// model a slow upstream deterministically under the paused clock.
    pub source_latency_ms: Arc<AtomicU64>,
    /// Count of upcoming gated source reads to fail. Decremented on each
    /// gated read — a self-clearing, countable fault, distinct from the
    /// one-shot [`on_load_chunk_error_once`](Self::on_load_chunk_error_once).
    pub source_fail_reads: Arc<AtomicU64>,
}
