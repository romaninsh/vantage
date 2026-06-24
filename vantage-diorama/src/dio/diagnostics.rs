//! Read-only introspection of a Dio: what's open, what's hydrated, how big the
//! cache is. Backed by the dedup scenery registry (so it's nearly free) — the
//! data behind a "Diorama inspector" panel and the assertion surface for tests.

use crate::dio::Dio;
use crate::scenery::table::RowStatusSummary;

/// A snapshot of one open table scenery.
#[derive(Debug, Clone)]
pub struct SceneryDiagnostic {
    /// The registry key: `(shape, conditions, sort, search, titles_only)`.
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
pub struct DioDiagnostics {
    /// Rows currently in the Dio's cache table.
    pub cache_rows: usize,
    /// Number of distinct two-pass query indexes built.
    pub query_indexes: usize,
    /// One entry per open (live) table scenery.
    pub sceneries: Vec<SceneryDiagnostic>,
}

impl DioDiagnostics {
    /// Total rows augmented to `Fresh` across all open sceneries.
    pub fn augmented_rows(&self) -> usize {
        self.sceneries.iter().map(|s| s.status.fresh).sum()
    }
}

impl Dio {
    /// Snapshot this Dio's live state for diagnostics: cache size, query-index
    /// count, and every open table scenery with its refcount and hydration
    /// breakdown. Prunes dead registry entries as a side effect.
    pub async fn diagnostics(&self) -> DioDiagnostics {
        let cache_rows = self.inner.cache.count().await.unwrap_or(0).max(0) as usize;
        let query_indexes = self.inner.query_indexes.lock().unwrap().len();

        let mut sceneries = Vec::new();
        {
            let mut reg = self.inner.table_sceneries.lock().unwrap();
            reg.retain(|_, weak| weak.strong_count() > 0);
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

        DioDiagnostics {
            cache_rows,
            query_indexes,
            sceneries,
        }
    }
}
