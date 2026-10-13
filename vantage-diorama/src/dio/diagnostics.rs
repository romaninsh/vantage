//! Read-only introspection of a Dio: what's open, what's hydrated, how big the
//! cache is. Backed by the dedup scenery registry (so it's nearly free) — the
//! data behind a "Diorama inspector" panel and the assertion surface for tests.

use vantage_core::{Context as _, Result, error};

use crate::dio::Dio;
use crate::scenery::table::RowStatusSummary;

/// A snapshot of one open table scenery.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SceneryDiagnostic {
    /// The registry key: `(shape, conditions, sort, titles_only)`.
    pub key: String,
    /// How many widgets currently hold this shared scenery.
    pub refcount: usize,
    /// Logical row count (a paged scenery reports its total, most unloaded).
    pub row_count: usize,
    /// Status breakdown over the rows actually materialized.
    pub status: RowStatusSummary,
}

/// A snapshot of a Dio's live state.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DioDiagnostics {
    /// The cache table this Dio reads and writes: the vista's name, plus `?`
    /// and the key of everything the Dio is narrowed by, when anything is.
    pub cache_table: String,
    /// Whether the Dio follows its master's change stream
    /// ([`DioBuilder::live`](crate::DioBuilder::live)).
    pub live: bool,
    /// Rows currently in the Dio's cache table.
    pub cache_rows: usize,
    /// Number of distinct two-pass query indexes built.
    pub query_indexes: usize,
    /// One entry per table scenery with a live handle. Two widgets sharing
    /// one deduplicated scenery are one entry, so `sceneries.len()` is the
    /// Dio's open scenery count, and it drops back once every handle is
    /// released.
    pub sceneries: Vec<SceneryDiagnostic>,
}

impl DioDiagnostics {
    /// Total rows augmented to `Fresh` across all open sceneries.
    pub fn augmented_rows(&self) -> usize {
        self.sceneries.iter().map(|s| s.status.fresh).sum()
    }
}

impl Dio {
    /// Snapshot this Dio's live state for diagnostics: its cache table and
    /// size, whether it follows live, the query-index count, and every open
    /// table scenery with its refcount and hydration breakdown. Reading never
    /// changes the Dio. Fails when the cache can't count its rows, so a broken
    /// cache never reads as an empty one.
    pub async fn diagnostics(&self) -> Result<DioDiagnostics> {
        let cache_rows = self.inner.cache.count().await.context(error!(
            "cache count failed",
            table = self.inner.cache_table_name
        ))?;
        let cache_rows = cache_rows.max(0) as usize;
        let query_indexes = self.inner.query_indexes.lock().unwrap().len();

        let mut sceneries = Vec::new();
        {
            let reg = self.inner.table_sceneries.lock().unwrap();
            for (key, weak) in reg.iter() {
                // strong_count before upgrade = the widgets' holds (our upgrade
                // would otherwise add one).
                let refcount = weak.strong_count();
                if let Some(scenery) = weak.upgrade() {
                    sceneries.push(SceneryDiagnostic {
                        key: key.clone(),
                        refcount,
                        row_count: scenery.row_count(),
                        status: scenery.status_summary(),
                    });
                }
            }
        }
        sceneries.sort_by(|a, b| a.key.cmp(&b.key));

        Ok(DioDiagnostics {
            cache_table: self.inner.cache_table_name.clone(),
            live: self.inner.live,
            cache_rows,
            query_indexes,
            sceneries,
        })
    }
}
