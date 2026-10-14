//! [`ChunkSink`]: where a paged loader pushes the rows it fetched.

mod flush;

use std::sync::{Arc, Weak};

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;

use crate::dio::pending::PendingFlashes;
use crate::lens::cache_backend::CacheTable;
use crate::scenery::EnrichedRecord;

pub(crate) use flush::FlushReport;

/// What a [`SceneryChunkTarget::write_chunk_row`] call did with the row it was
/// handed — the information a cancelled or failed load needs to put the
/// visible map back the way it found it.
#[derive(Debug)]
pub(crate) enum ChunkWrite {
    /// The row was written into the slot. `previous` is what the slot held
    /// before, or `None` when it was empty — the difference between "restore
    /// that record" and "remove the slot again" when the load is undone.
    Bound {
        previous: Option<Arc<EnrichedRecord>>,
    },
    /// Nothing was written: the client-side-sort hold-back or the
    /// identical-fresh-record dedup skipped this index, so it still holds
    /// whatever it held before the call and must not be undone.
    Skipped,
}

/// Implemented by the scenery a [`ChunkSink`] feeds, so the sink can bind a
/// fetched row into the right sparse-map slot without knowing the scenery's
/// state type.
pub(crate) trait SceneryChunkTarget: Send + Sync {
    /// Bind a freshly-fetched row into the scenery's sparse map. Returns
    /// [`ChunkWrite::Skipped`] when the client-side-sort hold-back or the
    /// identical-fresh-record dedup skipped the write, in which case `idx` was
    /// never touched; otherwise [`ChunkWrite::Bound`] carrying whatever the
    /// slot held before. Callers that track writes to undo (see
    /// [`restore_chunk_rows`](Self::restore_chunk_rows)) must record only the
    /// `Bound` ones — undoing a skipped index would overwrite a row this call
    /// never wrote.
    fn write_chunk_row(&self, idx: usize, id: String, record: Record<CborValue>) -> ChunkWrite;

    /// Record the grand total a chunk fetch reported. Returns whether the
    /// value moved, so the loader can decide to repaint.
    fn set_chunk_total(&self, total: usize) -> bool;

    /// Put these slots back the way `write_chunk_row` found them: `None`
    /// removes the slot (it was empty before), `Some(previous)` restores that
    /// record. Called when a chunk load that already bound rows is cancelled
    /// or fails before its buffered rows reach the cache — without this, a
    /// bound but never-committed row survives the load that produced it, and
    /// looks cached to anything that checks the visible map afterward, while
    /// simply *deleting* the slot would punch a hole where a good cached row
    /// was.
    fn restore_chunk_rows(&self, rows: &[(usize, Option<Arc<EnrichedRecord>>)]);
}

/// One pushed row awaiting the chunk's single cache write.
#[derive(Debug, Clone)]
pub(crate) struct ChunkRow {
    pub(crate) id: String,
    pub(crate) record: Record<CborValue>,
}

/// Rows a [`ChunkSink`] has accepted but not yet written, and which of them
/// `write_chunk_row` actually bound into the visible map.
#[derive(Default)]
pub(crate) struct Buffered {
    rows: Vec<ChunkRow>,
    /// One `(idx, previous)` entry per [`ChunkWrite::Bound`], in write order.
    /// A subset of `rows`' indices: the client-side-sort hold-back and the
    /// identical-fresh-record dedup buffer a row for the cache write without
    /// binding it, so those indices must not appear here — an undone load
    /// restores only these.
    bound: Vec<(usize, Option<Arc<EnrichedRecord>>)>,
}

type BufferedRows = Arc<std::sync::Mutex<Buffered>>;

/// Handed to a paged loader for each window. Each [`push`](Self::push) binds
/// one row to a row index of the requesting table scenery, and the rows reach
/// the Dio's cache in one write once the loader returns. Cheap to clone; the
/// scenery is held weakly, so dropping it mid-load makes later pushes fail.
#[derive(Clone)]
pub struct ChunkSink {
    pub(crate) target: Weak<dyn SceneryChunkTarget>,
    pub(crate) cache: Arc<dyn CacheTable>,
    /// Rows with a flash in flight — a chunk refetch is a reconcile, so
    /// its (possibly pre-write) snapshot must not clobber a staged value.
    pub(crate) pending: Arc<PendingFlashes>,
    /// Rows pushed so far, awaiting one write.
    ///
    /// A row per commit is a transaction — and a durability barrier — per row,
    /// so a hundred-row page costs a hundred of them. The rows are useless
    /// individually anyway: nothing observes the cache mid-load, and the
    /// scenery's visible map is bound by [`push`](Self::push) directly. Holding
    /// them and writing once turns a page load into a single transaction, which
    /// is also what lets a whole datasource share one store without each
    /// table's load queueing behind another's.
    pub(crate) buffer: BufferedRows,
    /// Whether the owning Dio's debug tap is on, captured at construction:
    /// [`flush_counted`](Self::flush_counted) pays for its extra counts only
    /// then.
    pub(crate) debug: bool,
}

impl std::fmt::Debug for ChunkSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChunkSink")
            .field("scenery_alive", &(self.target.upgrade().is_some()))
            .finish()
    }
}

impl ChunkSink {
    /// Report the grand total of matching rows, when the fetch that produced
    /// this chunk learned it (see [`Page::total`](vantage_vista::Page::total)).
    /// This is the only way a paged view learns its total. Optional: a loader
    /// that says nothing leaves the total as it was.
    pub fn set_total(&self, total: usize) {
        let Some(target) = self.target.upgrade() else {
            return;
        };
        target.set_chunk_total(total);
    }

    /// Bind one row to `idx` in the scenery's sparse map and buffer it for the
    /// cache. The row is visible to `row(idx)` as soon as `push` resolves; the
    /// scenery's generation bumps once the loader returns.
    pub async fn push(
        &self,
        idx: usize,
        id: impl Into<String>,
        record: Record<CborValue>,
    ) -> Result<()> {
        let id = id.into();
        let Some(target) = self.target.upgrade() else {
            return Err(vantage_core::error!("ChunkSink: scenery dropped"));
        };
        // A row with a flash in flight keeps its staged value: the slot
        // binds to what the cache holds, and the fetched (possibly
        // pre-write) snapshot is dropped. The row is deliberately NOT
        // buffered for the cache write — the cache already holds the staged
        // value, and writing the fetched snapshot over it is exactly what
        // this branch exists to prevent — but the bind is still recorded, so
        // an undone load puts the slot back rather than leaving this one
        // write behind.
        //
        // With nothing in the cache under this id (a flash staged for a row
        // this scenery has never fetched), there is no staged value to
        // prefer, so the fetched record is bound as-is: visible, but — like
        // the staged value beside it — not written to the cache by this load.
        // The flash's own commit is what persists the row.
        let record = if self.pending.contains(&id) {
            self.cache.get_value(&id).await?.unwrap_or(record)
        } else {
            if let Ok(mut buffer) = self.buffer.lock() {
                buffer.rows.push(ChunkRow {
                    id: id.clone(),
                    record: record.clone(),
                });
            }
            record
        };
        self.note_write(idx, target.write_chunk_row(idx, id, record));
        Ok(())
    }

    /// Remember a [`ChunkWrite::Bound`] so the load that made it can be undone.
    fn note_write(&self, idx: usize, write: ChunkWrite) {
        let ChunkWrite::Bound { previous } = write else {
            return;
        };
        if let Ok(mut buffer) = self.buffer.lock() {
            buffer.bound.push((idx, previous));
        }
    }

    /// The slots this chunk has actually bound into the visible map so far,
    /// each with what it held before — in write order. Not every pushed
    /// index: the client-side-sort hold-back and the identical-fresh-record
    /// dedup buffer a row for the cache write without binding it. Read by the
    /// loader when a load is cancelled mid-flight or fails, to restore exactly
    /// the rows this chunk bound but never got to commit — touching a skipped
    /// index would overwrite a row this chunk never wrote.
    pub(crate) fn bound_rows(&self) -> Vec<(usize, Option<Arc<EnrichedRecord>>)> {
        self.buffer
            .lock()
            .map(|buffer| buffer.bound.clone())
            .unwrap_or_default()
    }
}
