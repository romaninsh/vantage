use std::ops::Range;
use std::sync::Arc;

use crate::dio::DioInner;
use crate::lens::ChunkSink;
use crate::scenery::table::state::InFlightMarker;

/// What [`run_chunk_callback`](super::dispatch::run_chunk_callback) hands to
/// [`finish_chunk_load`](super::commit::finish_chunk_load) once the callback
/// has resolved — everything the commit and its bookkeeping need, carried
/// across the boundary the `select!` in
/// [`viewport_loop`](super::viewport_loop) cannot reach: by the time this
/// exists, the master has already answered, so nothing past this point may be
/// cancelled.
pub(super) struct PendingChunk {
    pub(super) dio_inner: Arc<DioInner>,
    pub(super) req: Option<u64>,
    pub(super) effective_range: Range<usize>,
    pub(super) effective_cached: usize,
    pub(super) first_hole: Option<usize>,
    pub(super) total_before: Option<usize>,
    pub(super) writer: ChunkSink,
    pub(super) result: vantage_core::Result<()>,
    pub(super) t: std::time::Instant,
    pub(super) force_load: bool,
    /// Held from the moment the range was claimed until `finish_chunk_load`
    /// returns, so the same-range guard in `run_chunk_callback` covers the
    /// whole load, not just its network half.
    pub(super) _in_flight: InFlightMarker,
}

/// Puts a chunk's bound-but-uncommitted rows back the way they were if its
/// callback's future is dropped before the callback returns — i.e. a newer
/// viewport superseded the load. Disarmed right after the callback returns,
/// whatever the result: a callback that ran to completion (successfully or
/// not) has its rows handled by the ordinary commit/error path in
/// `finish_chunk_load`, not by this guard.
pub(super) struct CancelOnDrop {
    pub(super) armed: bool,
    pub(super) writer: ChunkSink,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        restore_bound_rows(&self.writer);
    }
}

/// Undo every row this chunk bound into the visible map but never committed.
///
/// Only the slots `write_chunk_row` actually bound — not every pushed one. A
/// row the client-sort hold-back or the identical-fresh-record dedup skipped
/// was never written into the visible map, so touching it would rewrite a row
/// this chunk never changed (e.g. a `force_load` refresh over an unchanged,
/// already-cached range).
///
/// Shared by the cancel guard and the error path in `finish_chunk_load`: a
/// load that failed after pushing rows leaves them bound but uncached, which
/// is the same inconsistency a cancelled one leaves, so it unwinds the same
/// way.
pub(super) fn restore_bound_rows(writer: &ChunkSink) {
    let rows = writer.bound_rows();
    if rows.is_empty() {
        return;
    }
    if let Some(target) = writer.target.upgrade() {
        target.restore_chunk_rows(&rows);
    }
}
