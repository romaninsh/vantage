//! Read-call counters, shared by every clone of a [`MockShell`].

use std::sync::atomic::{AtomicUsize, Ordering};

use super::MockShell;

/// How many reads a [`MockShell`] and its clones have served (or refused) so
/// far, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MockCalls {
    pub list: usize,
    pub get: usize,
    pub count: usize,
    pub fetch_window: usize,
}

#[derive(Default)]
pub(super) struct Counters {
    list: AtomicUsize,
    get: AtomicUsize,
    count: AtomicUsize,
    fetch_window: AtomicUsize,
}

impl Counters {
    pub(super) fn list(&self) {
        self.list.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn get(&self) {
        self.get.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn count(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn fetch_window(&self) {
        self.fetch_window.fetch_add(1, Ordering::SeqCst);
    }
}

impl MockShell {
    /// The reads this shell and every clone of it have been asked for,
    /// refused ones included.
    pub fn calls(&self) -> MockCalls {
        let c = &self.calls;
        MockCalls {
            list: c.list.load(Ordering::SeqCst),
            get: c.get.load(Ordering::SeqCst),
            count: c.count.load(Ordering::SeqCst),
            fetch_window: c.fetch_window.load(Ordering::SeqCst),
        }
    }
}
