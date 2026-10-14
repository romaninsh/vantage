//! Read-only introspection of a Dio: what's open, what's hydrated, how big the
//! cache is. Backed by the live-view list (so it's nearly free) — the data
//! behind a "Diorama inspector" panel and the assertion surface for tests.

use vantage_core::{Context as _, Result, error};

use crate::dio::Dio;
use crate::scenery::table::RowStatusSummary;

/// A snapshot of one open table view.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SceneryDiagnostic {
    /// The key of the rows the view reads: its cache table, its query and
    /// whether it is titles-only.
    pub key: String,
    /// How many open views read these same rows, this one included.
    pub shared_by: usize,
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
    /// One entry per open table view, so `sceneries.len()` is the Dio's
    /// open view count, and it drops back as views are released. Views
    /// reading the same rows say so in `shared_by`.
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
    /// table view with its sharing and hydration breakdown. Reading never
    /// changes the Dio. Fails when the cache can't count its rows, so a broken
    /// cache never reads as an empty one.
    pub async fn diagnostics(&self) -> Result<DioDiagnostics> {
        let cache_rows = self.inner.cache.count().await.context(error!(
            "cache count failed",
            table = self.inner.cache_table_name
        ))?;
        let cache_rows = cache_rows.max(0) as usize;
        let query_indexes = self.inner.query_indexes.lock().unwrap().len();

        let mut sceneries: Vec<SceneryDiagnostic> = self
            .inner
            .live_views()
            .iter()
            .map(|view| {
                let shared = view.shared();
                SceneryDiagnostic {
                    key: shared.key.clone(),
                    shared_by: shared.views.lock().unwrap().len(),
                    row_count: shared.row_count(),
                    status: shared.status_summary(),
                }
            })
            .collect();
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
