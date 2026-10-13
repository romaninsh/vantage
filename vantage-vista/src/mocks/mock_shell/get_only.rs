//! A [`MockShell`] as a detail source keyed by id: `get` answers from the
//! rows, while `list`, `count` and `fetch_window` refuse. A get-only or
//! gated shell logs every `get` in dispatch order, and a gate can hold it
//! mid-flight, so a test can observe how a caller schedules its lookups.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Semaphore;
use vantage_core::{Result, error};

use super::MockShell;
use crate::capabilities::VistaCapabilities;

/// How a [`MockShell`] answers `get` and whether it lists at all. The log
/// is shared by every clone; the mode and the gate are carried by value.
#[derive(Clone, Default)]
pub(super) struct GetMode {
    only: bool,
    gate: Option<Arc<Semaphore>>,
    log: Arc<GetLog>,
}

#[derive(Default)]
struct GetLog {
    ids: Mutex<Vec<String>>,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
}

impl GetMode {
    /// On a get-only or gated shell, log the `get` of `id`, then wait for one
    /// permit of the gate, if any. The permit is consumed, so each one admits
    /// exactly one `get`. A plain shell's `get` passes straight through.
    pub(super) async fn admit(&self, id: &str) -> Result<()> {
        if !self.only && self.gate.is_none() {
            return Ok(());
        }
        let log = &self.log;
        log.ids.lock().unwrap().push(id.to_string());
        let now = log.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        log.max_in_flight.fetch_max(now, Ordering::SeqCst);
        let admitted = match &self.gate {
            Some(gate) => gate.acquire().await.map(|permit| permit.forget()),
            None => Ok(()),
        };
        log.in_flight.fetch_sub(1, Ordering::SeqCst);
        admitted.map_err(|_| error!("mock get gate is closed", id = id))
    }

    /// Refuses `verb` (a listing read) on a get-only shell.
    pub(super) fn refuse_listing(&self, verb: &str) -> Result<()> {
        if self.only {
            return Err(
                error!("a get-only mock shell answers only get", verb = verb)
                    .mark_unsupported()
                    .traced_debug(),
            );
        }
        Ok(())
    }
}

impl MockShell {
    /// `get` answers from the rows; `list`, `count` and `fetch_window`
    /// refuse as unsupported, rather than answering an empty page. A detail
    /// source keyed by id. Clears the capabilities, so search, order and
    /// traversal refuse as well; conditions still narrow what `get` finds.
    pub fn get_only(mut self) -> Self {
        self.gets.only = true;
        self.capabilities = VistaCapabilities::new();
        self
    }

    /// `get` waits on `gate` before answering: each `get` consumes one
    /// permit, so a gate created with no permits holds every `get` until the
    /// test calls `gate.add_permits`. For hydration-ordering tests.
    pub fn gated(mut self, gate: Arc<Semaphore>) -> Self {
        self.gets.gate = Some(gate);
        self
    }

    /// The ids of every `get` made through this shell or a clone of it, in
    /// dispatch order, including those still waiting on the gate. Empty
    /// unless the shell is [get-only](Self::get_only) or [gated](Self::gated).
    pub fn get_log(&self) -> Vec<String> {
        self.gets.log.ids.lock().unwrap().clone()
    }

    /// The most `get`s that were in flight at once.
    pub fn max_concurrent_gets(&self) -> usize {
        self.gets.log.max_in_flight.load(Ordering::SeqCst)
    }
}
