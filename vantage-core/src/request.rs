//! Request priority, carried as a tokio task-local: the owner of a wait sets
//! it with [`Priority::scope`], transports read it with [`Priority::current`].
//! A task spawned with `tokio::spawn` does not inherit it; the spawner must
//! wrap the spawned future in another `scope`.

use std::future::Future;

/// Whether someone is waiting on the result of the request about to be
/// sent. Transports choose their retry policy from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    /// Nobody is waiting: a poll over cached rows, a refresh, hydration.
    /// One attempt; a failure only feeds health.
    #[default]
    Background,
    /// Someone is waiting: a cold page, an uncached viewport, a write.
    /// Retry until the awaiting future is dropped.
    Essential,
}

tokio::task_local! {
    static PRIORITY: Priority;
}

impl Priority {
    /// The priority of the current task, `Background` when none was set.
    pub fn current() -> Priority {
        PRIORITY.try_with(|p| *p).unwrap_or_default()
    }

    pub fn is_essential(self) -> bool {
        matches!(self, Priority::Essential)
    }

    /// Run `fut` with this priority visible to everything it awaits.
    pub async fn scope<F: Future>(self, fut: F) -> F::Output {
        PRIORITY.scope(self, fut).await
    }
}
