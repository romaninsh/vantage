pub mod event_bus;
pub mod hot_tier;
pub mod impls;
pub mod refresh;
pub mod shell;
pub mod worker;

use std::sync::Arc;

use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::task::JoinHandle;
use vantage_vista::Vista;

use crate::lens::Lens;
use crate::ops::WriteOp;

pub use event_bus::DioEvent;
pub use hot_tier::HotTier;
pub use shell::DioShell;

/// Monotonically-increasing per-Scenery counter. Bumped on every state
/// change a Scenery exposes; UI adapters watch the receiver and
/// re-render on each bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Generation(pub u64);

impl From<u64> for Generation {
    fn from(v: u64) -> Self {
        Generation(v)
    }
}

impl From<Generation> for u64 {
    fn from(g: Generation) -> Self {
        g.0
    }
}

/// Per-entity binding of a Vista to a Lens.
///
/// Cheap to clone — wraps an `Arc<DioInner>` so all clones share the
/// same write queue, event bus, refresh task, and hot tier. Sceneries
/// keep their own `Arc<DioInner>` and remain alive as long as any
/// handle outlives the original Dio.
#[derive(Clone)]
pub struct Dio {
    pub(crate) inner: Arc<DioInner>,
}

pub(crate) struct DioInner {
    pub(crate) lens: Arc<Lens>,
    pub(crate) master: Vista,
    pub(crate) cache: Vista,
    pub(crate) cache_table_name: String,
    pub(crate) write_queue: mpsc::Sender<WriteOp>,
    pub(crate) event_bus: broadcast::Sender<DioEvent>,
    pub(crate) refresh_task: Mutex<Option<JoinHandle<()>>>,
    pub(crate) write_worker: Mutex<Option<JoinHandle<()>>>,
    pub(crate) hot_tier: Arc<HotTier>,
}

impl Dio {
    pub fn master(&self) -> &Vista {
        &self.inner.master
    }

    pub fn cache(&self) -> &Vista {
        &self.inner.cache
    }

    pub fn cache_table_name(&self) -> &str {
        &self.inner.cache_table_name
    }

    /// Subscribe to the Dio's internal event bus. Sceneries call this
    /// in their `subscribe` impl; user callbacks may also call it to
    /// observe cross-Dio reactions.
    pub fn subscribe(&self) -> broadcast::Receiver<DioEvent> {
        self.inner.event_bus.subscribe()
    }
}
