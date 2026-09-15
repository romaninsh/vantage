//! What a client tells the outside world about its traffic. The consumer
//! (the desktop app's health registry) keys everything by the `key` given to
//! the builder — the datasource name — and the crate never learns what a
//! datasource is.

use std::time::Duration;

use super::ClientError;

#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// One attempt is about to be sent.
    Started,
    /// One attempt got a `2xx`. `bytes` is the `Content-Length`, when sent.
    Succeeded { status: u16, ms: u64, bytes: Option<u64> },
    /// One attempt failed. The last `Failed` of a call carries the error the
    /// call returns.
    Failed { error: ClientError, ms: u64 },
    /// A retry will run after `after`; `attempt` is its 1-based number.
    RetryScheduled { after: Duration, attempt: usize },
    BreakerOpened { cooldown: Duration },
    BreakerClosed,
    /// Reported by the caller once it has parsed a response.
    RowsPulled { n: usize },
    /// Reported by the caller once a write was acknowledged.
    WritePushed,
}

pub trait TransportObserver: Send + Sync {
    fn on_event(&self, key: &str, event: TransportEvent);
}
