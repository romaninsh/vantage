//! Committing a chunk's buffered rows to the cache in one write.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;

use super::ChunkSink;

/// What [`ChunkSink::flush_counted`] committed: how many rows, and (tap
/// permitting) the cache's row count before and after, plus the field-name
/// union across the batch and its encoded size. The source of the "cache
/// write" and "columns" debug lines.
///
/// Everything but `written` is empty or `0` unless the sink's tap is on: it
/// exists only for those lines.
#[derive(Debug, Clone, Default)]
pub(crate) struct FlushReport {
    pub(crate) written: usize,
    pub(crate) cache_rows_before: i64,
    pub(crate) cache_rows_after: i64,
    pub(crate) columns_received: Vec<String>,
    pub(crate) payload_bytes: usize,
}

impl ChunkSink {
    /// Commit everything pushed so far, in one write, and report what was
    /// written.
    ///
    /// The loader calls this once the paged loader returns, and before
    /// anything reads the cache back (the post-load re-sort rebuilds the
    /// visible map from it), or the load appears to have fetched nothing.
    ///
    /// The before/after counts are taken only when `self.debug` is set: with
    /// the tap off nothing reads them.
    pub(crate) async fn flush_counted(&self) -> Result<FlushReport> {
        let rows: indexmap::IndexMap<String, Record<CborValue>> = {
            let Ok(mut buffer) = self.buffer.lock() else {
                return Ok(FlushReport::default());
            };
            std::mem::take(&mut buffer.rows)
                .into_iter()
                .map(|row| (row.id, row.record))
                .collect()
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

/// Field-name union (insertion order, deduped) and total encoded size across
/// a batch of records — the raw material of the "columns" debug line.
///
/// The dedup goes through an `IndexSet` rather than a `Vec` scanned with
/// `contains`: a page of wide rows is rows × columns keys, and a linear scan
/// per key would make that quadratic in the columns, the one case the line
/// exists to make visible.
pub(crate) fn wide_data_signals<'a>(
    records: impl Iterator<Item = &'a Record<CborValue>>,
) -> (Vec<String>, usize) {
    let mut columns: indexmap::IndexSet<String> = indexmap::IndexSet::new();
    let mut payload_bytes = 0usize;
    for record in records {
        for key in record.keys() {
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
