//! The guards that stop the rows' work: the task set aborted with the last
//! view, and the in-flight load marker.

use std::sync::{Arc, Mutex};

use tokio::task::JoinHandle;

use super::SharedRows;

/// Aborts the tasks it holds on [`abort_all`](Self::abort_all) and on drop.
/// Once aborted it stays closed: a task handed to it later is aborted at
/// once, so nothing the rows were doing starts again.
#[derive(Default)]
pub(crate) struct TaskGuard(Mutex<Tasks>);

#[derive(Default)]
pub(crate) struct Tasks {
    running: Vec<JoinHandle<()>>,
    closed: bool,
}

impl TaskGuard {
    pub(crate) fn hold(&self, task: JoinHandle<()>) {
        let mut tasks = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if tasks.closed {
            task.abort();
            return;
        }
        tasks.running.retain(|task| !task.is_finished());
        tasks.running.push(task);
    }

    pub(crate) fn abort_all(&self) {
        let mut tasks = self.0.lock().unwrap_or_else(|e| e.into_inner());
        tasks.closed = true;
        for task in tasks.running.drain(..) {
            task.abort();
        }
    }
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.abort_all();
    }
}

/// Clears `load_in_flight` when dropped, so a load that is cancelled
/// mid-flight — its future dropped by the viewport loop — cannot leave its
/// range marked as still loading and make the next request for that range
/// skip itself. Owns the `Arc` rather than borrowing it so it can travel
/// inside `PendingChunk` from `run_chunk_callback` to `finish_chunk_load`,
/// holding the guard for the whole load rather than just its network half.
pub(crate) struct InFlightMarker(pub(crate) Arc<SharedRows>);

impl Drop for InFlightMarker {
    fn drop(&mut self) {
        *self.0.load_in_flight.lock().unwrap() = None;
        self.0.publish_view_stats();
    }
}
