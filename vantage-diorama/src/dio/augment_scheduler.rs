//! Central augment scheduler — one flight per row, fair across views.
//!
//! Every consumer that wants rows hydrated — a scenery's viewport, a facade
//! read blocking on its window — registers a requester queue via
//! [`AugmentScheduler::ticket`] and enqueues row ids into it. A small pool of
//! worker tasks (default one, see
//! [`LensBuilder::augment_workers`](crate::lens::LensBuilder::augment_workers))
//! drains the queues **round-robin across requesters**, so two views with
//! disjoint viewports interleave instead of one starving the other, and the
//! fetch order under a single worker is deterministic.
//!
//! Dedup is total: a popped id already in flight is dropped (its completion
//! serves every waiter), and the worker re-checks the cache before fetching,
//! so an id another requester already hydrated costs one cache read and zero
//! fetches. Dropping a ticket withdraws its queued ids — a closing view stops
//! pulling — while a fetch already in flight runs to completion and lands in
//! the cache: paid-for work is kept.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, Weak};

use indexmap::IndexMap;
use tokio::sync::{Notify, oneshot};

use super::DioInner;
use crate::DioEvent;

pub(crate) struct AugmentScheduler {
    state: Mutex<SchedState>,
    work_notify: Notify,
}

#[derive(Default)]
struct SchedState {
    next_requester: u64,
    /// One FIFO queue per live requester; the map's insertion order is the
    /// round-robin ring. Empty queues stay registered (skipped for free)
    /// until their ticket drops.
    queues: IndexMap<u64, VecDeque<String>>,
    /// The requester the last job was popped from. The next scan starts
    /// AFTER it — keyed by requester rather than ring position, so a view
    /// that registers mid-flight gets served next instead of waiting out
    /// another full turn of the earlier queues.
    last_served: Option<u64>,
    /// Ids a worker currently owns — the global single-flight set.
    in_flight: HashSet<String>,
    /// Per-id completion waiters (the facade's blocking reads). Keyed by id,
    /// not requester: whichever fetch settles the id fires them all.
    waiters: HashMap<String, Vec<oneshot::Sender<Result<(), String>>>>,
}

impl AugmentScheduler {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(SchedState::default()),
            work_notify: Notify::new(),
        }
    }

    /// Register a requester. The ticket enqueues into its own FIFO queue and
    /// withdraws it on drop.
    pub(crate) fn ticket(self: &Arc<Self>) -> AugmentTicket {
        let id = {
            let mut s = self.state.lock().unwrap();
            s.next_requester += 1;
            let id = s.next_requester;
            s.queues.insert(id, VecDeque::new());
            id
        };
        AugmentTicket {
            sched: self.clone(),
            id,
        }
    }

    /// Pop the next id round-robin and claim it in `in_flight`. Ids popped
    /// while already in flight are discarded — their one completion fires the
    /// waiters. `None` when every queue is empty.
    fn next_job(&self) -> Option<String> {
        let mut s = self.state.lock().unwrap();
        loop {
            if s.queues.is_empty() {
                return None;
            }
            let len = s.queues.len();
            let start = s
                .last_served
                .and_then(|rid| s.queues.get_index_of(&rid))
                .map(|pos| pos + 1)
                .unwrap_or(0);
            let mut popped = None;
            for step in 0..len {
                let pos = (start + step) % len;
                let entry = {
                    let (rid, queue) = s.queues.get_index_mut(pos).unwrap();
                    queue.pop_front().map(|id| (*rid, id))
                };
                if let Some((rid, id)) = entry {
                    s.last_served = Some(rid);
                    popped = Some(id);
                    break;
                }
            }
            let id = popped?;
            if s.in_flight.contains(&id) {
                continue;
            }
            s.in_flight.insert(id.clone());
            return Some(id);
        }
    }

    /// Release the in-flight claim and fire every waiter registered for `id`.
    fn complete(&self, id: &str, result: Result<(), String>) {
        let senders = {
            let mut s = self.state.lock().unwrap();
            s.in_flight.remove(id);
            s.waiters.remove(id)
        };
        for tx in senders.into_iter().flatten() {
            let _ = tx.send(result.clone());
        }
    }

    fn notify_work(&self) {
        // Wake every parked worker, and leave one permit for a worker that
        // is between `next_job` and `notified().await` — it consumes the
        // permit and loops back to find the work.
        self.work_notify.notify_waiters();
        self.work_notify.notify_one();
    }
}

/// A requester's handle into the scheduler. Dropping it withdraws every id
/// still sitting in its queue (and any waiter that no longer has a path to
/// completion) — this is how a closing scenery stops pulling.
pub(crate) struct AugmentTicket {
    sched: Arc<AugmentScheduler>,
    id: u64,
}

impl AugmentTicket {
    /// Fire-and-forget: queue `ids` for hydration. Ids this requester already
    /// queued, and ids currently in flight, are skipped — the recheck before
    /// each fetch makes re-enqueueing safe but pointless.
    pub(crate) fn enqueue(&self, ids: impl IntoIterator<Item = String>) {
        let mut queued = false;
        {
            let mut s = self.sched.state.lock().unwrap();
            for id in ids {
                if s.in_flight.contains(&id) {
                    continue;
                }
                let Some(queue) = s.queues.get_mut(&self.id) else {
                    return;
                };
                if queue.contains(&id) {
                    continue;
                }
                queue.push_back(id);
                queued = true;
            }
        }
        if queued {
            self.sched.notify_work();
        }
    }

    /// Queue `ids` and block until every one has settled — the facade path.
    /// Ids already in flight are not re-queued; their waiter fires when the
    /// running fetch completes. Returns the first failure.
    pub(crate) async fn enqueue_and_wait(&self, ids: Vec<String>) -> Result<(), String> {
        let mut receivers = Vec::with_capacity(ids.len());
        {
            let mut s = self.sched.state.lock().unwrap();
            for id in ids {
                let (tx, rx) = oneshot::channel();
                s.waiters.entry(id.clone()).or_default().push(tx);
                receivers.push(rx);
                if s.in_flight.contains(&id) {
                    continue;
                }
                let Some(queue) = s.queues.get_mut(&self.id) else {
                    return Err("augment scheduler ticket withdrawn".to_string());
                };
                if !queue.contains(&id) {
                    queue.push_back(id);
                }
            }
        }
        self.sched.notify_work();
        let mut first_err = None;
        for rx in receivers {
            match rx.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    first_err.get_or_insert(e);
                }
                Err(_) => {
                    first_err.get_or_insert("augment scheduler shut down".to_string());
                }
            }
        }
        match first_err {
            None => Ok(()),
            Some(e) => Err(e),
        }
    }
}

impl Drop for AugmentTicket {
    fn drop(&mut self) {
        let mut s = self.sched.state.lock().unwrap();
        let Some(withdrawn) = s.queues.shift_remove(&self.id) else {
            return;
        };
        // An id whose only path to completion was this queue will never be
        // popped — drop its waiters so nothing awaits forever. Ids still
        // queued elsewhere or in flight keep theirs.
        for id in withdrawn {
            if s.in_flight.contains(&id) || s.queues.values().any(|q| q.contains(&id)) {
                continue;
            }
            s.waiters.remove(&id);
        }
    }
}

/// One worker: pop → hydrate → complete, parking on the notify while idle.
/// Holds only a `Weak` to the Dio so it never keeps it alive; the Dio aborts
/// these tasks on drop (a parked worker would otherwise idle forever).
pub(crate) async fn augment_worker_loop(dio: Weak<DioInner>, sched: Arc<AugmentScheduler>) {
    loop {
        let Some(id) = sched.next_job() else {
            if dio.strong_count() == 0 {
                return;
            }
            sched.work_notify.notified().await;
            continue;
        };
        let Some(inner) = dio.upgrade() else {
            return;
        };
        let result = crate::dio::augment_passes::hydrate_one(&inner, &id).await;
        let result = match result {
            Ok(()) => Ok(()),
            Err(e) => {
                let error = e.to_string();
                let _ = inner.event_bus.send(DioEvent::RecordLoadFailed {
                    id: id.clone(),
                    error: error.clone(),
                });
                Err(error)
            }
        };
        drop(inner);
        sched.complete(&id, result);
    }
}
