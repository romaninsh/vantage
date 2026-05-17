use std::sync::Arc;

use tokio::sync::watch;

use crate::dio::Generation;

use super::enriched_record::EnrichedRecord;

#[derive(Debug, Clone)]
pub enum RecordStatus {
    Fresh,
    Stale,
    Loading,
    NotFound,
    Error(String),
}

/// Reactive view onto a single record by id within a Dio.
pub trait RecordScenery: Send + Sync {
    fn record(&self) -> Option<Arc<EnrichedRecord>>;
    fn status(&self) -> RecordStatus;

    fn request_refresh(&self);
    fn subscribe(&self) -> watch::Receiver<Generation>;
}
