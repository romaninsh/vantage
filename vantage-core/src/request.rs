//! Request priority: whether someone is waiting on the result of the
//! request that is about to be sent.
//!
//! The value travels as a tokio task-local, so the code that owns the wait
//! (a viewport loader, a cold page open, an outbox job) sets it once with
//! [`Priority::scope`] and every request awaited inside — however many
//! crates down — reads it with [`Priority::current`]. Nothing in between
//! needs a parameter. Only the owner of the wait may set it; transports
//! only read it. A task spawned with `tokio::spawn` does not inherit it:
//! the spawner must wrap the spawned future in another `scope`.

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

#[cfg(test)]
mod tests {
    use super::Priority;

    #[test]
    fn default_is_background() {
        assert_eq!(Priority::default(), Priority::Background);
        assert!(!Priority::Background.is_essential());
        assert!(Priority::Essential.is_essential());
    }

    #[tokio::test]
    async fn current_is_background_outside_any_scope() {
        assert_eq!(Priority::current(), Priority::Background);
    }

    #[test]
    fn current_outside_a_runtime_is_background() {
        // No tokio runtime at all here, not just no `scope` — `current()`
        // must not panic when there is no task to hold the task-local.
        assert_eq!(Priority::current(), Priority::Background);
    }

    #[tokio::test]
    async fn scope_sets_the_priority_for_nested_awaits() {
        async fn deep() -> Priority {
            tokio::task::yield_now().await;
            Priority::current()
        }
        let seen = Priority::Essential.scope(async { deep().await }).await;
        assert_eq!(seen, Priority::Essential);
        assert_eq!(Priority::current(), Priority::Background, "scope ended");
    }

    #[tokio::test]
    async fn inner_scope_overrides_outer() {
        let seen = Priority::Essential
            .scope(async {
                Priority::Background
                    .scope(async { Priority::current() })
                    .await
            })
            .await;
        assert_eq!(seen, Priority::Background);
    }

    #[tokio::test]
    async fn spawned_tasks_do_not_inherit() {
        let seen = Priority::Essential
            .scope(async { tokio::spawn(async { Priority::current() }).await.unwrap() })
            .await;
        assert_eq!(seen, Priority::Background);
    }
}
