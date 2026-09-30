//! One compiled def, and the state every sim thread shares.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use vantage_memory::{MemoryStore, MemoryTableHandle};
use vantage_rhai::rhai::{AST, Engine, Map as RhaiMap};

use super::SimDef;
use super::clock::SimClock;
use super::sched::Sched;
use super::stats::Counters;
use crate::FakerColumn;
use crate::generator::Memo;

/// Least real time between two error logs of one def.
const ERROR_LOG_EVERY: Duration = Duration::from_secs(60);

/// One def, compiled and bound to its clock.
pub(super) struct Kind {
    pub def: SimDef,
    pub ast: Arc<AST>,
    pub clock: SimClock,
    /// The spawner's args as a Rhai map.
    pub args: RhaiMap,
    pub errors: Mutex<ErrorLog>,
    /// `row()`'s state per table, shared by every sim spawned from this def,
    /// so a one-sim-per-row def (`builtin:fifo`) still walks a `walk` or
    /// even-spread `date` generator forward across sims instead of every
    /// sim rebuilding it from row zero.
    pub(super) row_state: Mutex<HashMap<String, Arc<RowTableState>>>,
}

/// `row()`'s state for one table: the next sequence number, and the
/// positional generators' memo (walk series, tree plans) built from it.
/// Growing the memo at index `seq` only ever reads index `seq - 1`, so it
/// makes no difference which sim's call grows it first; the seq and the
/// memo can be, and are, guarded separately.
#[derive(Default)]
pub(super) struct RowTableState {
    seq: AtomicUsize,
    pub(super) memo: Mutex<Memo>,
}

impl RowTableState {
    pub(super) fn next_seq(&self) -> usize {
        self.seq.fetch_add(1, Ordering::Relaxed)
    }
}

#[derive(Default)]
pub(super) struct ErrorLog {
    last: Option<Instant>,
    suppressed: u64,
}

impl Kind {
    /// Log a failed sim: the first failure of this def, then at most one
    /// line per [`ERROR_LOG_EVERY`] carrying the count of the ones skipped.
    pub fn report(&self, error: &str) {
        let mut log = self.errors.lock().unwrap_or_else(|e| e.into_inner());
        if log.last.is_some_and(|at| at.elapsed() < ERROR_LOG_EVERY) {
            log.suppressed += 1;
            return;
        }
        let suppressed = std::mem::take(&mut log.suppressed);
        log.last = Some(Instant::now());
        tracing::error!(sim = %self.def.name, suppressed, %error, "faker sim failed; it ended");
    }

    /// `row()`'s shared state for `table`: the next sequence number, and
    /// the memo it grows. One state per table, kept for the def's lifetime.
    pub fn row_state(&self, table: &str) -> Arc<RowTableState> {
        self.row_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(table.to_string())
            .or_insert_with(|| Arc::new(RowTableState::default()))
            .clone()
    }
}

/// Everything the sim threads share.
pub(super) struct Inner {
    pub engine: Arc<Engine>,
    pub kinds: Vec<Kind>,
    pub by_name: HashMap<String, usize>,
    pub store: MemoryStore,
    /// Store tables the verbs have resolved, by name.
    pub tables: RwLock<HashMap<String, MemoryTableHandle>>,
    /// A table's declared columns, for the `row()` verb. A table with no
    /// entry here has no declared columns, and `row()` on it is empty.
    pub columns: HashMap<String, Vec<FakerColumn>>,
    pub sched: Sched,
    pub seed: Option<u64>,
    /// Wall time the engine started at; every sim clock meets it there.
    pub origin: f64,
    pub handles: Mutex<Vec<JoinHandle<()>>>,
    pub counters: Counters,
    /// Called with (def name, error message) for every sim that ends in
    /// error, in addition to the def's own rate-limited log.
    pub on_error: Option<Arc<OnSimError>>,
}

/// Callback taking (def name, error message).
pub type OnSimError = dyn Fn(&str, &str) + Send + Sync;

impl Inner {
    /// Table `name` of the store, if it exists. Never creates one. Found
    /// tables are cached; a store never loses a table.
    pub fn table(&self, name: &str) -> Option<MemoryTableHandle> {
        if let Some(t) = self
            .tables
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(name)
        {
            return Some(t.clone());
        }
        if !self.store.table_names().iter().any(|n| n == name) {
            return None;
        }
        let t = self.store.table(name);
        self.tables
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.to_string(), t.clone());
        Some(t)
    }

    /// Mute (or unmute) every store table's broadcasts — during the warm
    /// start. Unmuting a table written while quiet sends one `Reset`.
    pub fn set_quiet(&self, quiet: bool) {
        for name in self.store.table_names() {
            self.store.table(&name).set_quiet(quiet);
        }
    }

    /// Stop every sim, wait for their threads to end and join them. The
    /// driver thread, if any, is the caller's to join.
    pub fn stop_sims(&self) {
        self.sched.stop();
        self.sched.wait_no_threads();
        let handles = std::mem::take(&mut *self.handles.lock().unwrap_or_else(|e| e.into_inner()));
        for handle in handles {
            let _ = handle.join();
        }
    }
}
