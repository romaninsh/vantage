use std::sync::{Arc, Weak};

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;

use crate::dio::pending::PendingFlashes;
use crate::lens::cache_backend::CacheTable;

/// Trait the calling Scenery implements so a `ChunkSink` can stuff a
/// freshly-fetched row into the right sparse-map slot. Decouples
/// `ChunkSink` (which lives in `lens`) from the concrete scenery
/// state type.
pub trait SceneryChunkTarget: Send + Sync {
    fn write_chunk_row(&self, idx: usize, id: String, record: Record<CborValue>);

    /// Record the grand total a chunk fetch reported. Returns whether the
    /// value moved, so the loader can decide to repaint.
    fn set_chunk_total(&self, total: usize) -> bool;
}

/// Rows a [`ChunkSink`] has accepted but not yet written — see
/// [`ChunkSink::buffer`].
type BufferedRows = Arc<std::sync::Mutex<Vec<(String, Record<CborValue>)>>>;

/// Handle passed to `on_load_chunk` callbacks. Each [`push`](Self::push)
/// writes one row to the Dio's cache and binds it to a row index in
/// the calling `TableScenery`'s sparse map. Cheap to clone; the scenery
/// is held by `Weak`, so dropping it mid-load makes subsequent pushes
/// fail cleanly.
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
    /// The owning Dio's debug tap state, captured at construction.
    ///
    /// `flush_counted` reads this to decide whether the extra before/after
    /// `count()` calls it does purely to report the "cache write" debug line
    /// are worth paying for. With the tap off, nothing reads that report, so
    /// the counts would be pure overhead — the one thing the debug stream is
    /// not allowed to cost a datasource that never opted in.
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
    /// this chunk also learned it.
    ///
    /// Paged sources carry the total in the same response as the window (see
    /// [`Vista::fetch_window_counted`](vantage_vista::Vista::fetch_window_counted)),
    /// so a lens that reads it here needs no separate `total_provider` — and
    /// therefore no second round trip on open. Calling this is optional; a
    /// lens that says nothing leaves the total exactly as it was.
    pub fn set_total(&self, total: usize) {
        let Some(target) = self.target.upgrade() else {
            return;
        };
        target.set_chunk_total(total);
    }

    /// Insert one row into the cache and bind it to `idx` in the
    /// scenery's sparse map. The row is visible to `row(idx)` as soon
    /// as `push` resolves, but the scenery's generation only bumps
    /// once `on_load_chunk` returns (via the `RangeLoaded` emission).
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
        // pre-write) snapshot is dropped.
        if self.pending.contains(&id) {
            let staged = self.cache.get_value(&id).await?.unwrap_or(record);
            target.write_chunk_row(idx, id, staged);
            return Ok(());
        }
        if let Ok(mut buffer) = self.buffer.lock() {
            buffer.push((id.clone(), record.clone()));
        }
        target.write_chunk_row(idx, id, record);
        Ok(())
    }

    /// Commit everything pushed so far, in one write, and report a
    /// [`FlushReport`] of how many rows were written and — tap permitting —
    /// the cache's row count before and after, the numbers behind the
    /// "cache write" debug line.
    ///
    /// Called by the loader once `on_load_chunk` returns — and it must run
    /// before anything reads the cache back (the post-load re-sort rebuilds the
    /// visible map from it), or the load appears to have fetched nothing.
    ///
    /// The before/after counts are skipped (and reported as `0`) unless
    /// `self.debug` is set: they exist only for that debug line, so with the
    /// tap off they'd be pure overhead nothing observes.
    pub(crate) async fn flush_counted(&self) -> Result<FlushReport> {
        let rows: indexmap::IndexMap<String, Record<CborValue>> = {
            let Ok(mut buffer) = self.buffer.lock() else {
                return Ok(FlushReport::default());
            };
            std::mem::take(&mut *buffer).into_iter().collect()
        };
        if rows.is_empty() {
            return Ok(FlushReport::default());
        }
        let written = rows.len();
        if !self.debug {
            self.cache.insert_values(rows).await?;
            return Ok(FlushReport {
                written,
                ..Default::default()
            });
        }
        // The wide-data signal for the "columns" debug line: the union of
        // field names across this chunk and the total CBOR-encoded size.
        // Computed here (buffer already collected, one pass) rather than
        // per-`push`, and only reached because `self.debug` is set — a sink
        // built with the tap off never pays for the encoding.
        let (columns_received, payload_bytes) = wide_data_signals(rows.values());
        let cache_rows_before = self.cache.count().await?;
        self.cache.insert_values(rows).await?;
        let cache_rows_after = self.cache.count().await?;
        Ok(FlushReport {
            written,
            cache_rows_before,
            cache_rows_after,
            columns_received,
            payload_bytes,
        })
    }
}

/// Field-name union (insertion order, deduped) and total ciborium-encoded
/// size across a batch of buffered records — the raw material for the
/// "columns" debug line's `received_count`/`received_sample`/`payload_bytes`.
/// Dedup goes through an `IndexSet` (O(1) membership, insertion order
/// preserved on iteration) rather than a `Vec` scanned with `contains`: a
/// full page of wide rows is exactly rows × columns, and a linear scan per
/// key would make that quadratic in the columns — the one case this line
/// exists to make visible.
fn wide_data_signals<'a>(
    records: impl Iterator<Item = &'a Record<CborValue>>,
) -> (Vec<String>, usize) {
    let mut columns: indexmap::IndexSet<String> = indexmap::IndexSet::new();
    let mut payload_bytes = 0usize;
    for record in records {
        for key in record.keys() {
            // `insert` is a no-op (and doesn't reorder) when the key is
            // already present, so no separate `contains` check is needed.
            columns.insert(key.clone());
        }
        let map: Vec<(CborValue, CborValue)> = record
            .iter()
            .map(|(k, v)| (CborValue::Text(k.clone()), v.clone()))
            .collect();
        let mut buf = Vec::new();
        if ciborium::into_writer(&CborValue::Map(map), &mut buf).is_ok() {
            payload_bytes += buf.len();
        }
    }
    (columns.into_iter().collect(), payload_bytes)
}

/// The result of `ChunkSink::flush_counted` — how many rows a chunk write
/// committed, and (tap permitting) the cache's row count before and after,
/// plus the wide-data signal: the field-name union across the batch and its
/// total encoded size. Source for the "cache write" debug line's
/// `written`/`new`/`updated`/`cached_rows` fields and the "columns" line's
/// `received_count`/`received_sample`/`payload_bytes`.
///
/// `columns_received` and `payload_bytes` are empty/`0` unless the sink's
/// tap is on — they exist only for the "columns" line, so with the tap off
/// they'd be pure overhead nothing observes.
#[derive(Debug, Clone, Default)]
pub struct FlushReport {
    pub written: usize,
    pub cache_rows_before: i64,
    pub cache_rows_after: i64,
    pub columns_received: Vec<String>,
    pub payload_bytes: usize,
}

/// One row's worth of payload — exposed publicly for callers that
/// want to model the same shape (queueing rows for testing, etc.).
#[derive(Debug, Clone)]
pub struct ChunkRow {
    pub idx: usize,
    pub id: String,
    pub record: Record<CborValue>,
}
