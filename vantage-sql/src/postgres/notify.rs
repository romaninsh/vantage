//! One `LISTEN` connection per data source, shared by every subscriber.
//!
//! A `PgListener` holds a pool connection for as long as it lives, so one
//! per watched vista starves the pool after a dozen open lists. The hub runs
//! a single listener on its own task: subscribers ask it to `LISTEN` on
//! their channels and receive every notification's channel name over one
//! broadcast, keeping the ones they asked for.

use std::collections::HashSet;
use std::sync::Arc;

use sqlx::postgres::{PgListener, PgPool};
use tokio::sync::{broadcast, mpsc, oneshot};

/// Notifications buffered per subscriber. A subscriber that falls further
/// behind sees `Lagged`, which still means "something changed".
const BUFFER: usize = 1024;

#[derive(Clone, Debug)]
pub(crate) enum HubEvent {
    /// A notification arrived on this channel.
    Notified(Arc<str>),
    /// The listener stopped for good; every subscriber's feed is over.
    Failed(Arc<str>),
}

type ListenRequest = (Vec<String>, oneshot::Sender<Result<(), Arc<str>>>);

#[derive(Clone)]
pub(crate) struct NotifyHub {
    requests: mpsc::UnboundedSender<ListenRequest>,
    events: broadcast::Sender<HubEvent>,
}

impl NotifyHub {
    /// Spawn the listener task. Needs a Tokio runtime.
    pub(crate) fn spawn(pool: PgPool) -> Self {
        let (requests, rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(BUFFER);
        tokio::spawn(run(pool, rx, events.clone()));
        Self { requests, events }
    }

    /// False once the listener task has ended; the data source then spawns
    /// a fresh hub for the next subscriber.
    pub(crate) fn is_running(&self) -> bool {
        !self.requests.is_closed()
    }

    /// `LISTEN` on `channels` and return the event feed. Resolves after the
    /// `LISTEN` ran, and the feed is opened before it, so no notification
    /// sent after this returns is missed.
    pub(crate) async fn subscribe(
        &self,
        channels: Vec<String>,
    ) -> Result<broadcast::Receiver<HubEvent>, Arc<str>> {
        let feed = self.events.subscribe();
        let (ack, done) = oneshot::channel();
        let stopped = || Arc::<str>::from("notification listener stopped");
        self.requests.send((channels, ack)).map_err(|_| stopped())?;
        done.await.map_err(|_| stopped())??;
        Ok(feed)
    }
}

async fn run(
    pool: PgPool,
    mut requests: mpsc::UnboundedReceiver<ListenRequest>,
    events: broadcast::Sender<HubEvent>,
) {
    let mut listener = match PgListener::connect_with(&pool).await {
        Ok(listener) => listener,
        Err(e) => return fail(requests, &events, format!("open pg listener: {e}")),
    };
    let mut listened = HashSet::new();
    loop {
        // `recv` is cancel-safe: sqlx consumes a message only once it is
        // read whole, and a reconnect cut short starts over on the next call.
        tokio::select! {
            request = requests.recv() => {
                // Every handle is gone: the data source was dropped.
                let Some((channels, ack)) = request else { return };
                let fresh: Vec<String> = channels
                    .into_iter()
                    .filter(|c| !listened.contains(c))
                    .collect();
                let result = listener
                    .listen_all(fresh.iter().map(String::as_str))
                    .await
                    .map_err(|e| Arc::<str>::from(format!("LISTEN {}: {e}", fresh.join(", "))));
                if result.is_ok() {
                    listened.extend(fresh);
                }
                let _ = ack.send(result);
            }
            // A lost connection is reconnected and re-LISTENed inside
            // `recv`, which then carries on; only what it cannot recover
            // from (the pool closed or timed out) comes back as an error.
            notification = listener.recv() => match notification {
                Ok(n) => {
                    let _ = events.send(HubEvent::Notified(n.channel().into()));
                }
                Err(e) => return fail(requests, &events, format!("notification listener: {e}")),
            },
        }
    }
}

/// End the hub: refuse queued and later requests, and tell every
/// subscriber once.
fn fail(
    mut requests: mpsc::UnboundedReceiver<ListenRequest>,
    events: &broadcast::Sender<HubEvent>,
    message: String,
) {
    let message: Arc<str> = message.into();
    requests.close();
    while let Ok((_, ack)) = requests.try_recv() {
        let _ = ack.send(Err(message.clone()));
    }
    let _ = events.send(HubEvent::Failed(message));
}
