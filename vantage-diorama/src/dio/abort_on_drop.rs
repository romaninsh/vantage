//! A join handle that aborts its task when dropped.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::task::JoinHandle;

/// Ties a spawned task to its owner: dropping the guard aborts the task, so
/// work nobody waits for, or that serves something already gone, stops.
/// Awaiting it awaits the task.
pub(crate) struct AbortOnDrop<T = ()>(pub(crate) JoinHandle<T>);

impl<T> Future for AbortOnDrop<T> {
    type Output = std::result::Result<T, tokio::task::JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::AbortOnDrop;

    #[tokio::test]
    async fn dropping_the_guard_aborts_the_task() {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let guard = AbortOnDrop(tokio::spawn(async move {
            let _tx = tx;
            std::future::pending::<()>().await;
        }));
        drop(guard);
        // The never-finishing task only releases its sender when it is aborted.
        let closed = tokio::time::timeout(Duration::from_secs(5), rx)
            .await
            .expect("task was not aborted");
        assert!(closed.is_err());
    }
}
