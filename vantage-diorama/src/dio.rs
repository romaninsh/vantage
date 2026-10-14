pub(crate) mod augment_passes;
pub(crate) mod augment_scheduler;
mod augmented;
mod builder;
mod derive;
mod detail_state;
#[cfg(feature = "diagnostics")]
mod diagnostics;
mod event_bus;
mod fetch;
pub(crate) mod flash;
mod import;
mod ingress;
mod inner;
mod open;
pub(crate) mod pending;
mod plan;
pub(crate) mod query_index;
mod refresh;
mod residual;
pub(crate) mod shell;
mod watch;
mod whole_copy;
pub(crate) mod worker;

use std::sync::Arc;

use tokio::sync::broadcast;
#[cfg(feature = "test-util")]
use tokio::task::JoinHandle;
use vantage_vista::{Vista, VistaQuery};

pub(crate) use augmented::AugmentedClause;
pub use builder::DioBuilder;
pub(crate) use builder::DioParts;
pub(crate) use detail_state::DetailState;
#[cfg(feature = "diagnostics")]
pub use diagnostics::{DioDiagnostics, SceneryDiagnostic};
pub use event_bus::DioEvent;
pub use fetch::Fetched;
pub use import::ImportOutcome;
pub(crate) use inner::{DioInner, shared_key};
pub(crate) use plan::Plan;
pub(crate) use residual::admitted_by_dio;
pub(crate) use shell::DioShell;
pub(crate) use watch::Follower;

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

/// Per-entity binding of a Vista to a Lens, built once by
/// [`Lens::dio`](crate::Lens::dio) and immutable after.
///
/// Cheap to clone — wraps an `Arc<DioInner>` so all clones share the
/// same write queue, event bus, and refresh task. Narrowing never changes a
/// Dio: [`with_query`](Self::with_query) and its siblings derive a new one.
///
/// The Dio lives until its last strong holder drops: `Dio` clones, an open
/// form's [`Servo`](crate::Servo), a facade from [`cached`](Self::cached), or a
/// queued write still landing. Sceneries hold only a `Weak` and don't extend
/// it. When it ends, its live follow ends with it and drops its stream.
#[derive(Clone)]
pub struct Dio {
    pub(crate) inner: Arc<DioInner>,
}

/// A non-owning handle to a [`Dio`] — the currency for registries
/// (memoization maps, inspection routes) that must *observe* a Dio
/// without keeping its pipeline alive. Upgrade to use; a failed upgrade
/// means every strong holder (open pages, sceneries) has released it.
#[derive(Clone)]
pub struct WeakDio {
    inner: std::sync::Weak<DioInner>,
}

impl Dio {
    /// Downgrade to a non-owning [`WeakDio`].
    pub fn downgrade(&self) -> WeakDio {
        WeakDio {
            inner: Arc::downgrade(&self.inner),
        }
    }
}

impl WeakDio {
    /// Reclaim a usable [`Dio`] if any strong handle is still alive.
    pub fn upgrade(&self) -> Option<Dio> {
        self.inner.upgrade().map(|inner| Dio { inner })
    }
}

/// The Dio's *effective* write capabilities — the one gate UI chrome
/// asks before offering add/edit/delete.
///
/// Defaults to the master Vista's own capabilities; a Lens
/// [`writer`](crate::LensBuilder::writer) lifts all three, because the
/// writer — not the master — then takes the writes (a read-only CSV becomes editable, with
/// changes landing wherever the route sends them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteCapabilities {
    pub can_insert: bool,
    pub can_update: bool,
    pub can_delete: bool,
}

impl Dio {
    /// The master Vista, narrowed by every clause it accepted.
    pub fn master(&self) -> &Vista {
        &self.inner.master
    }

    /// The clauses the master refused, or never saw because they name an
    /// augmented column. Diorama evaluates them over the rows it holds.
    pub fn residual(&self) -> &VistaQuery {
        &self.inner.residual
    }

    /// The master's id column, resolved when the Dio was built. Every cached
    /// row is keyed by it.
    pub fn id_column(&self) -> &str {
        &self.inner.id_column
    }

    /// The debug tap this Dio inherited from its Lens. Enabled when the
    /// datasource opted into the `vantage_diorama::debug` stream.
    #[cfg(feature = "diagnostics")]
    pub fn debug_tap(&self) -> crate::debug::DebugTap {
        self.inner.lens.debug.clone()
    }

    /// Subscribe to the Dio's internal event bus. Sceneries call this
    /// in their `subscribe` impl; user callbacks may also call it to
    /// observe cross-Dio reactions.
    pub fn subscribe_events(&self) -> broadcast::Receiver<DioEvent> {
        self.inner.event_bus.subscribe()
    }

    /// Take the per-Dio write worker's `JoinHandle` out of the inner
    /// state. Returns `Some` on the first call, `None` afterwards.
    ///
    /// Once taken, the worker is no longer owned by the Dio — it keeps
    /// running until the last `Sender` (held by `DioInner`) drops, at
    /// which point the loop's `recv()` returns `None` and the task
    /// completes. Callers can `await` the returned handle to observe
    /// that clean shutdown.
    ///
    /// For test harnesses asserting the worker's lifecycle.
    #[cfg(feature = "test-util")]
    pub async fn take_write_worker_handle(&self) -> Option<JoinHandle<()>> {
        self.inner.write_worker.lock().await.take()
    }

    /// The Dio's effective write capabilities — master caps, lifted to
    /// fully writable when the Lens has a writer. UI chrome
    /// gates its add/edit/delete affordances on this, nothing else.
    pub fn write_capabilities(&self) -> WriteCapabilities {
        self.inner.write_capabilities()
    }
}
