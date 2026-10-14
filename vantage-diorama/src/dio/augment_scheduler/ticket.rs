use std::sync::Arc;

use tokio::sync::oneshot;

use super::AugmentScheduler;

/// A requester's handle into the scheduler. Dropping it withdraws every id
/// still sitting in its queue (and any waiter that no longer has a path to
/// completion) — this is how a closing scenery stops pulling.
pub(crate) struct AugmentTicket {
    pub(super) sched: Arc<AugmentScheduler>,
    pub(super) id: u64,
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

    /// Whether this requester has nothing queued (a fetch it asked for may
    /// still be in flight).
    pub(crate) fn is_idle(&self) -> bool {
        let s = self.sched.state.lock().unwrap();
        s.queues.get(&self.id).is_none_or(|queue| queue.is_empty())
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
