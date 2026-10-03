//! Drive async Vista calls from synchronous Rhai code.

use std::future::Future;
use std::task::{Context, Poll, Waker};

use vantage_core::{Result, error};

/// Run `fut` to completion from synchronous script code. With a tokio
/// runtime handle, uses it (callers evaluate under `spawn_blocking`).
/// Without one, polls on this thread; a future still pending then errors.
pub fn block_on<F: Future>(fut: F) -> Result<F::Output> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return Ok(handle.block_on(fut));
    }
    let mut fut = std::pin::pin!(fut);
    match fut.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(out) => Ok(out),
        Poll::Pending => Err(error!(
            "this data source needs a tokio runtime; run the script under spawn_blocking"
        )),
    }
}
