//! Long-lived background tokio runtime for sqlx pool operations.
//!
//! The cucumber harness in `tests/bdd.rs` runs on a `current_thread` runtime
//! with `start_paused = true` — required by the refresh scenarios that drive
//! `tokio::time::advance` against virtual time. Under paused time, the
//! runtime auto-advances the clock whenever no task is runnable, which
//! collides with sqlx 0.8's `Pool::acquire` wrapping its wait loop in
//! `tokio::time::timeout(acquire_timeout, …)`: a brief gap between the
//! previous `PoolConnection`'s spawn-on-Drop `return_to_pool` task and the
//! next acquire's poll is enough for the runtime to leap virtual time
//! forward 30 seconds and fire `Error::PoolTimedOut`. Aborted
//! `return_to_pool` tasks at runtime shutdown also leak the pool's size
//! counter and lock the pool out of future connections (issue #260).
//!
//! Routing every sqlx pool operation through a separate, real-time runtime
//! sidesteps both problems: sqlx's timers tick against wall-clock time and
//! the runtime never goes away mid-test. Callers `dispatch(fut).await`
//! to send a future to the io-runtime and await its result back on the
//! main runtime.

use std::future::Future;
use std::sync::OnceLock;
use std::thread;

use tokio::runtime::{Builder, Handle};
use tokio::sync::oneshot;

static SQLITE_RUNTIME: OnceLock<Handle> = OnceLock::new();

fn handle() -> &'static Handle {
    SQLITE_RUNTIME.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        thread::Builder::new()
            .name("vantage-diorama-bdd-sqlite-io".into())
            .spawn(move || {
                let rt = Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("build sqlite-io runtime");
                tx.send(rt.handle().clone())
                    .expect("send io-runtime handle");
                // Park the runtime forever — the process exit tears it down.
                rt.block_on(std::future::pending::<()>());
            })
            .expect("spawn sqlite-io runtime thread");
        rx.recv().expect("receive io-runtime handle")
    })
}

/// Drive `fut` to completion on the long-lived io-runtime, returning the
/// value to the caller's runtime. The future must be `Send + 'static` so
/// it can cross the thread boundary; its output is sent back over a
/// oneshot.
pub async fn dispatch<F, T>(fut: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    handle().spawn(async move {
        let _ = tx.send(fut.await);
    });
    rx.await.expect("sqlite-io task panicked or was cancelled")
}
