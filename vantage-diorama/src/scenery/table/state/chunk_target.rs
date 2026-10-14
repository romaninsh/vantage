//! Viewport chunk writes: the sink a chunk load pushes rows into.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use ciborium::Value as CborValue;
use vantage_types::{Record, cbor_id_to_string};

use crate::lens::{ChunkWrite, SceneryChunkTarget};
use crate::scenery::enriched_record::{EnrichedRecord, RowStatus};

use super::SharedRows;

impl SceneryChunkTarget for SharedRows {
    /// A total learned from the chunk fetch itself. This is how a paged lens
    /// sizes the scrollbar without a separate count — i.e. without a second
    /// request on open, which is the one network call `open()` used to await.
    fn set_chunk_total(&self, total: usize) -> bool {
        self.load_total_reported.store(true, Ordering::SeqCst);
        self.total_ever_stated.store(true, Ordering::SeqCst);
        self.set_total(Some(total))
    }

    fn write_chunk_row(&self, idx: usize, id: String, record: Record<CborValue>) -> ChunkWrite {
        // Count every received row (before the skips below), so the loader can
        // tell a short page (end of set) from a full one.
        self.load_push_count.fetch_add(1, Ordering::SeqCst);
        // A window answers the view only while the view serves windows: a load
        // landing after its query moved to the cache or failed writes the
        // cache, never the visible map. A view serving windows has its order
        // pushed, so the window is already in the view's order and is written
        // straight through.
        if !self.is_chunk_loaded() {
            return ChunkWrite::Skipped;
        }
        // Skip the write entirely when this slot already holds the same fresh
        // record: a refresh that re-fetches identical data must not look like a
        // change. Only a new/!Fresh slot or a different record is "dirty", and
        // only a dirty load bumps the generation (see `loader::commit`).
        {
            let rows = self.rows.read().unwrap();
            if let Some(existing) = rows.get(&idx)
                && existing.status == RowStatus::Fresh
                && existing.record == record
            {
                return ChunkWrite::Skipped;
            }
        }
        let enriched = Arc::new(EnrichedRecord::fresh(record));
        let previous = self.rows.write().unwrap().insert(idx, enriched);
        self.write_id_to_idx(|id_to_idx| id_to_idx.insert(id, idx));
        self.load_dirty.store(true, Ordering::SeqCst);
        ChunkWrite::Bound { previous }
    }

    /// Deliberately does NOT bump the generation. It only ever runs as the
    /// undo of a load that never became visible as a load: the rows it puts
    /// back are the rows the grid is already painting, so there is nothing new
    /// to show. Bumping would repaint the identical frame — and, on the cancel
    /// path, would do so in the middle of the superseding load that is about
    /// to bump for real.
    ///
    /// Applied newest-first so a slot written more than once by the same load
    /// ends up holding what it held before the load's *first* write, not what
    /// that first write replaced.
    fn restore_chunk_rows(&self, entries: &[(usize, Option<Arc<EnrichedRecord>>)]) {
        if entries.is_empty() {
            return;
        }
        let touched: std::collections::HashSet<usize> = entries.iter().map(|(i, _)| *i).collect();
        {
            let mut rows = self.rows.write().unwrap();
            for (idx, previous) in entries.iter().rev() {
                match previous {
                    Some(record) => {
                        rows.insert(*idx, record.clone());
                    }
                    None => {
                        rows.remove(idx);
                    }
                }
            }
        }
        // `id_to_idx` is keyed by id, not index, so the slots this undid are
        // found by value rather than looked up directly. Every mapping into a
        // touched slot goes, including the one the undone load added; the
        // record put back re-registers below under its own id, read through
        // the same `cbor_id_to_string` every other cache key goes through, so
        // an integer id column maps as readily as a text one. A row whose
        // record does not embed the master's id column at all loses its
        // mapping until the next load rebinds the slot — an in-place
        // `RecordChanged` update for it is skipped, which is a miss the next
        // fetch corrects, where a mapping left pointing at a row that is no
        // longer there would write the wrong record into a live slot.
        // `rows` before `id_to_idx`, the order every writer takes them in.
        let rows = self.rows.read().unwrap();
        let mut id_to_idx = self.id_to_idx.write().unwrap();
        id_to_idx.retain(|_, idx| !touched.contains(idx));
        self.forget_idx_to_id();
        let Some(dio) = self.dio_weak.upgrade() else {
            return;
        };
        for idx in &touched {
            if let Some(record) = rows.get(idx)
                && let Some(id) = record
                    .record
                    .get(&dio.id_column)
                    .and_then(cbor_id_to_string)
            {
                id_to_idx.insert(id, *idx);
            }
        }
    }
}
