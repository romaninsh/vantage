//! Live push from the master Vista into the cache, for as long as the Dio
//! lives.

mod batch;

use std::sync::Arc;

use vantage_core::Result;

use super::Dio;
use batch::Tail;

impl Dio {
    /// Start watching the master Vista and apply every change it pushes.
    ///
    /// Transparent by design: if the master advertises
    /// [`Subscribe`](vantage_vista::Capability::Subscribe) (SurrealDB LIVE, Postgres
    /// `LISTEN/NOTIFY`), this subscribes and pipes each
    /// [`VistaChange`](vantage_vista::VistaChange) through
    /// [`apply_change`](Self::apply_change) on a background task — no polling;
    /// changes that arrive together share one cache transaction. If
    /// it doesn't, this is a no-op and the caller's `refresh_every` timer keeps
    /// the cache fresh instead. Either way the reactive stack behaves the same;
    /// only the freshness mechanism differs.
    ///
    /// Failure is self-correcting rather than silent — critical, since the task
    /// is detached and can't return an error:
    /// - a change that fails to apply triggers a full [`refresh`](Self::refresh)
    ///   so the cache can't drift out of step with the backend;
    /// - if the subscription drops (a WebSocket close ends the stream), the task
    ///   backs off, **re-subscribes**, and refreshes to close the gap of changes
    ///   missed while disconnected — it does not silently stop pushing.
    ///
    /// The initial subscription is established synchronously so a
    /// misconfiguration surfaces to the caller here. The background task holds
    /// only a `Weak` to the Dio and returns as soon as the last `Dio` handle
    /// drops — even mid-wait on a stream that never yields, or mid back-off —
    /// dropping the stream with it, which releases the backend's subscription.
    pub async fn watch(&self) -> Result<()> {
        let master = self.master();
        if !master
            .capabilities()
            .has(vantage_vista::Capability::Subscribe)
        {
            return Ok(());
        }
        let mut stream = master.watch().await?;
        let weak = Arc::downgrade(&self.inner);
        let mut cancel = self.inner.watch_cancel.subscribe();

        tokio::spawn(async move {
            use futures::StreamExt;
            let base = std::time::Duration::from_millis(200);
            let mut backoff = base;

            'session: loop {
                // Drain the current subscription.
                loop {
                    let Some(next) = until_dio_drops(&mut cancel, stream.next()).await else {
                        return;
                    };
                    let Some(item) = next else { break };
                    let Some(inner) = weak.upgrade() else { return };
                    let dio = Dio { inner };
                    let first = match item {
                        Ok(change) => change,
                        Err(e) => {
                            tracing::warn!(error = %e, "watch: stream error; resubscribing");
                            break;
                        }
                    };
                    let (changes, tail) = batch::ready_batch(first, &mut stream);
                    match dio.apply_changes(changes).await {
                        Ok(()) => backoff = base,
                        Err(e) => {
                            // A change failed to apply — the cache may be out
                            // of step for its rows. Reconcile the whole set so
                            // the view can't drift silently; when the stream
                            // also ended, the resubscribe below reconciles.
                            tracing::error!(error = %e, "watch: apply failed; reconciling");
                            if tail.is_none() {
                                let _ = dio.refresh().await;
                            }
                        }
                    }
                    match tail {
                        None => {}
                        Some(Tail::Ended) => break,
                        Some(Tail::Failed(e)) => {
                            tracing::warn!(error = %e, "watch: stream error; resubscribing");
                            break;
                        }
                    }
                }

                // The subscription ended (a dropped connection yields `None`).
                // Push is gone; reconnect with backoff instead of exiting, then
                // reconcile to recover anything missed during the gap.
                loop {
                    if until_dio_drops(&mut cancel, tokio::time::sleep(backoff))
                        .await
                        .is_none()
                    {
                        return;
                    }
                    backoff = (backoff * 2).min(std::time::Duration::from_secs(30));
                    // Subscribe without holding the Dio, so it can still drop
                    // while the backend is slow to answer.
                    let Some(master) = weak.upgrade().map(|inner| Dio { inner }.master()) else {
                        return;
                    };
                    let Some(subscribed) = until_dio_drops(&mut cancel, master.watch()).await
                    else {
                        return;
                    };
                    match subscribed {
                        Ok(fresh) => {
                            stream = fresh;
                            let Some(inner) = weak.upgrade() else { return };
                            let _ = Dio { inner }.refresh().await;
                            tracing::info!("watch: resubscribed and reconciled");
                            continue 'session;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "watch: resubscribe failed; retrying");
                        }
                    }
                }
            }
        });
        Ok(())
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
