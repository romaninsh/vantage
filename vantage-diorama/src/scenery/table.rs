use std::ops::Range;
use std::sync::Arc;

use tokio::sync::watch;

use crate::dio::Generation;

use super::enriched_record::EnrichedRecord;

/// UI-side sort direction. Mirrors `vantage_vista::SortDirection` but
/// kept distinct so Scenery callers don't need to import vista types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Reactive view onto a Dio that exposes an ordered, paginated row set.
///
/// Stage 1 declares the trait surface; the concrete [`TableSceneryState`]
/// and the builder land in stage 5.
pub trait TableScenery: Send + Sync {
    fn row_count(&self) -> usize;
    fn has_more(&self) -> bool;
    fn estimated_total(&self) -> Option<usize>;
    fn row(&self, idx: usize) -> Option<Arc<EnrichedRecord>>;

    fn set_viewport(&self, range: Range<usize>);
    fn request_load_more(&self);
    fn request_refresh(&self);
    fn set_search(&self, query: Option<String>);
    fn set_sort(&self, column: Option<String>, dir: SortDir);

    fn subscribe(&self) -> watch::Receiver<Generation>;
}
