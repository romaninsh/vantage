//! A view's change signals, and the hook its [`SharedRows`](super::super::state::SharedRows)
//! holds to reach them.
//!
//! Rows change in the `SharedRows`, but each view is subscribed to on its
//! own: a view that re-points to other rows keeps its subscribers, and two
//! views over one set of rows never see each other's view-only changes (the
//! shown range, a re-point). So the generation and view-stats channels live
//! on the view, and the `SharedRows` fans each bump out to the views
//! attached to it.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use tokio::sync::watch;

use crate::dio::Generation;
use crate::dio::augment_scheduler::AugmentTicket;

/// One view's generation (rows moved) and view-stats (snapshot moved)
/// channels.
pub(crate) struct ViewSignal {
    generation: AtomicU64,
    generation_tx: watch::Sender<Generation>,
    stats_generation: AtomicU64,
    stats_tx: watch::Sender<Generation>,
}

impl ViewSignal {
    pub(crate) fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            generation_tx: watch::channel(Generation::default()).0,
            stats_generation: AtomicU64::new(0),
            stats_tx: watch::channel(Generation::default()).0,
        }
    }

    /// The view's rows moved. `send_replace`, so a consumer subscribing later
    /// still reads the latest generation, even with no receiver alive now.
    pub(crate) fn bump(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.generation_tx.send_replace(Generation(next));
    }

    /// The view's [`ViewStats`](super::super::ViewStats) moved.
    pub(crate) fn bump_stats(&self) {
        let next = self.stats_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.stats_tx.send_replace(Generation(next));
    }

    pub(crate) fn subscribe(&self) -> watch::Receiver<Generation> {
        self.generation_tx.subscribe()
    }

    pub(crate) fn subscribe_stats(&self) -> watch::Receiver<Generation> {
        self.stats_tx.subscribe()
    }
}

/// What a `SharedRows` knows of one view attached to it.
pub(crate) struct ViewHook {
    /// Weak: the rows never keep a closed view's subscribers alive.
    pub(crate) signal: Weak<ViewSignal>,
    /// The rows this view last asked to load: what a refresh re-fetches and
    /// a change of serving refills. `None` until it asks for any.
    pub(crate) viewport: Option<Range<usize>>,
    /// The view's place in the augment scheduler's round robin; a two-pass
    /// viewport hydrates under it. `None` off two-pass.
    pub(crate) ticket: Option<Arc<AugmentTicket>>,
}

impl ViewHook {
    pub(crate) fn new(signal: &Arc<ViewSignal>, ticket: Option<Arc<AugmentTicket>>) -> Self {
        Self {
            signal: Arc::downgrade(signal),
            viewport: None,
            ticket,
        }
    }
}
