use ciborium::Value as CborValue;
use tokio::sync::watch;

use crate::dio::Generation;

#[derive(Debug, Clone)]
pub enum ValueStatus {
    Fresh,
    Stale,
    Loading,
    Error(String),
}

/// Reactive view onto a single scalar — typically an aggregate
/// (`COUNT`, `SUM`) computed against the underlying Vista.
pub trait ValueScenery: Send + Sync {
    fn value(&self) -> Option<CborValue>;
    fn status(&self) -> ValueStatus;

    fn request_refresh(&self);
    fn subscribe(&self) -> watch::Receiver<Generation>;
}
