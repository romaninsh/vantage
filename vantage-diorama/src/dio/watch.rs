//! Live follow: push from the master Vista into the cache, for as long as the
//! Dio lives.

mod admit;
mod batch;

use std::sync::{Arc, Weak};
use std::time::Duration;

use futures::StreamExt;
use vantage_core::Result;
use vantage_vista::VistaChangeStream;

use super::{Dio, DioInner};
use admit::Admit;
use batch::Tail;

/// A subscription to the master's change stream, made but not yet followed.
///
/// Subscribing and following are separate so a Dio can subscribe before its
/// eager start copies the set and follow once the copy has landed: the stream
/// holds every change made during the copy, and applying them after it means
/// a snapshot taken before a change can't overwrite it.
pub(crate) struct Follower {
    stream: VistaChangeStream,
    admit: Admit,
    dio: Weak<DioInner>,
    cancel: tokio::sync::watch::Receiver<()>,
}

impl Dio {
    /// Subscribe to the master Vista's change stream, for a Dio built with
    /// [`DioBuilder::live`](crate::DioBuilder::live). `None` when the master
    /// doesn't advertise [`Subscribe`](vantage_vista::Capability::Subscribe):
    /// the Lens's `Refresher::every` timer keeps the cache fresh instead.
    ///
    /// The subscription is made before this returns, so a misconfiguration
    /// fails the build.
    pub(crate) async fn subscribe(&self) -> Result<Option<Follower>> {
        let master = self.master();
        if !master
            .capabilities()
            .has(vantage_vista::Capability::Subscribe)
        {
            return Ok(None);
        }
        Ok(Some(Follower {
            stream: master.watch().await?,
            admit: Admit::for_dio(&self.inner),
            dio: Arc::downgrade(&self.inner),
            cancel: self.inner.watch_cancel.subscribe(),
        }))
    }
}

impl Follower {
    /// Apply every change the master pushes (SurrealDB LIVE, Postgres
    /// `LISTEN/NOTIFY`), with no polling; changes that arrive together share
    /// one cache transaction.
    ///
    /// Each change is held against the Dio's query first (see [`Admit`]): a
    /// row that falls outside it applies as a delete, and over a query with
    /// native conditions every change re-reads the set.
    ///
    /// Failure is self-correcting rather than silent, which matters because
    /// the task is detached and can't return an error:
    /// - a change that fails to apply triggers a full
    ///   [`refresh`](Dio::refresh), which re-reads the set, so the cache
    ///   can't drift out of step with the backend;
    /// - if the subscription drops (a WebSocket close ends the stream), the
    ///   task backs off, **re-subscribes**, and refreshes to close the gap of
    ///   changes missed while disconnected;
    /// - a re-read that fails is logged, published as `LoadFailed`, and
    ///   retried with backoff until one succeeds.
    ///
    /// Holds only a `Weak` to the Dio and returns as soon as the last `Dio`
    /// handle drops, even mid-wait on a stream that never yields or mid
    /// back-off, dropping the stream with it, which releases the backend's
    /// subscription.
    pub(crate) async fn run(mut self) {
        let base = Duration::from_millis(200);
        let mut backoff = base;
        loop {
            if !self.drain(&mut backoff, base).await {
                return;
            }
            if !self.resubscribe(&mut backoff).await {
                return;
            }
        }
    }

    /// Apply the current subscription until it ends. `false` once the Dio
    /// is gone.
    async fn drain(&mut self, backoff: &mut Duration, base: Duration) -> bool {
        loop {
            let Some(next) = until_dio_drops(&mut self.cancel, self.stream.next()).await else {
                return false;
            };
            let Some(item) = next else { return true };
            let first = match item {
                Ok(change) => change,
                Err(e) => {
                    tracing::warn!(error = %e, "watch: stream error; resubscribing");
                    return true;
                }
            };
            let (applied, tail) =
                batch::apply_ready(&self.dio, first, &mut self.stream, &self.admit).await;
            match applied {
                Ok(()) => *backoff = base,
                // A change failed to apply, so the cache may be out of step
                // for its rows. Re-read the whole set so the view can't drift
                // silently; when the stream also ended, the resubscribe
                // re-reads.
                Err(e) => {
                    tracing::error!(
                        table = %self.table(),
                        error = %e,
                        "watch: apply failed; re-reading the set",
                    );
                    if tail.is_none() && !self.reread(backoff).await {
                        return false;
                    }
                }
            }
            match tail {
                None => {}
                Some(Tail::Dropped) => return false,
                Some(Tail::Ended) => return true,
                Some(Tail::Failed(e)) => {
                    tracing::warn!(error = %e, "watch: stream error; resubscribing");
                    return true;
                }
            }
        }
    }

    /// The subscription ended (a dropped connection yields `None`). Reconnect
    /// with backoff, then re-read the set to recover anything missed during
    /// the gap. `false` once the Dio is gone.
    async fn resubscribe(&mut self, backoff: &mut Duration) -> bool {
        loop {
            if until_dio_drops(&mut self.cancel, tokio::time::sleep(*backoff))
                .await
                .is_none()
            {
                return false;
            }
            *backoff = (*backoff * 2).min(Duration::from_secs(30));
            // Subscribe without holding the Dio, so it can still drop while
            // the backend is slow to answer.
            let Some(master) = self.dio.upgrade().map(|inner| inner.master.clone()) else {
                return false;
            };
            let Some(subscribed) = until_dio_drops(&mut self.cancel, master.watch()).await else {
                return false;
            };
            match subscribed {
                Ok(fresh) => {
                    self.stream = fresh;
                    return self.reread(backoff).await;
                }
                Err(e) => tracing::warn!(
                    table = %self.table(),
                    error = %e,
                    "watch: resubscribe failed; retrying",
                ),
            }
        }
    }

    /// Re-read the set, retrying with backoff until a re-read succeeds: the
    /// cache may be out of step with the master until one does, and a
    /// refresh is never a silent no-op. Each failure is logged and
    /// published as [`DioEvent::LoadFailed`](super::DioEvent::LoadFailed),
    /// so a view can show it. Changes arriving meanwhile wait in the stream
    /// and apply after. `false` once the Dio is gone.
    async fn reread(&mut self, backoff: &mut Duration) -> bool {
        loop {
            let Some(inner) = self.dio.upgrade() else {
                return false;
            };
            let table = inner.master.name().to_string();
            let reread = Dio {
                inner: inner.clone(),
            }
            .refresh()
            .await;
            match reread {
                Ok(()) => {
                    tracing::info!(table = %table, "watch: re-read the set");
                    return true;
                }
                Err(e) => {
                    tracing::error!(
                        table = %table,
                        error = %e,
                        retry_in = ?*backoff,
                        "watch: re-reading the set failed; the cache may be out of step until a retry succeeds",
                    );
                    let _ = inner.event_bus.send(super::DioEvent::LoadFailed {
                        range: 0..0,
                        error: e.to_string(),
                    });
                }
            }
            drop(inner);
            if until_dio_drops(&mut self.cancel, tokio::time::sleep(*backoff))
                .await
                .is_none()
            {
                return false;
            }
            *backoff = (*backoff * 2).min(Duration::from_secs(30));
        }
    }

    /// The master's name, for the log, or an empty one once the Dio is gone.
    fn table(&self) -> String {
        self.dio
            .upgrade()
            .map(|inner| inner.master.name().to_string())
            .unwrap_or_default()
    }
}

/// Run `fut` unless the Dio drops first. `None` means it dropped and `fut`
/// was abandoned. Nothing is ever sent on the channel, so `changed()` only
/// resolves when the sender closes with the last Dio handle.
async fn until_dio_drops<F: std::future::Future>(
    cancel: &mut tokio::sync::watch::Receiver<()>,
    fut: F,
) -> Option<F::Output> {
    use futures::future::{Either, select};
    let fut = std::pin::pin!(fut);
    let dropped = std::pin::pin!(cancel.changed());
    match select(fut, dropped).await {
        Either::Left((output, _)) => Some(output),
        Either::Right(_) => None,
    }
}
