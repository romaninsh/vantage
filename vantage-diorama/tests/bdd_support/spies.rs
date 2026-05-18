use std::sync::Arc;
use std::sync::atomic::AtomicU64;

#[derive(Clone, Default)]
pub struct Spies {
    pub on_start: Arc<AtomicU64>,
    pub on_refresh: Arc<AtomicU64>,
    pub on_event: Arc<AtomicU64>,
    pub on_write: Arc<AtomicU64>,
}
