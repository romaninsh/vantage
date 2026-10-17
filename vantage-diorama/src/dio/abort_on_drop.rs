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
