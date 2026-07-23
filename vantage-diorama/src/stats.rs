//! Live-instance counters for leak diagnosis.
//!
//! Each counted type owns a [`Tally`] field: constructing the type
//! increments its counter, dropping it decrements. [`live_counts`] snapshots
//! all of them — an embedder can log it periodically to verify that closing
//! a page really releases its Dios and sceneries instead of accumulating
//! them.

use std::sync::atomic::{AtomicUsize, Ordering};

static DIOS: AtomicUsize = AtomicUsize::new(0);
static TABLE_SCENERIES: AtomicUsize = AtomicUsize::new(0);
static RECORD_SCENERIES: AtomicUsize = AtomicUsize::new(0);
static SERVOS: AtomicUsize = AtomicUsize::new(0);

/// RAII counter handle — one per counted instance, embedded as a field so
/// every construction/drop path is covered automatically.
#[derive(Debug)]
pub(crate) struct Tally(&'static AtomicUsize);

impl Tally {
    fn claim(counter: &'static AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Tally(counter)
    }

    pub(crate) fn dio() -> Self {
        Self::claim(&DIOS)
    }

    pub(crate) fn table_scenery() -> Self {
        Self::claim(&TABLE_SCENERIES)
    }

    pub(crate) fn record_scenery() -> Self {
        Self::claim(&RECORD_SCENERIES)
    }

    pub(crate) fn servo() -> Self {
        Self::claim(&SERVOS)
    }
}

impl Drop for Tally {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Point-in-time census of live diorama objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveCounts {
    pub dios: usize,
    pub table_sceneries: usize,
    pub record_sceneries: usize,
    pub servos: usize,
}

/// Snapshot the live-instance counters.
pub fn live_counts() -> LiveCounts {
    LiveCounts {
        dios: DIOS.load(Ordering::Relaxed),
        table_sceneries: TABLE_SCENERIES.load(Ordering::Relaxed),
        record_sceneries: RECORD_SCENERIES.load(Ordering::Relaxed),
        servos: SERVOS.load(Ordering::Relaxed),
    }
}
