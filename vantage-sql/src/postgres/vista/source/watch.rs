//! Change feed over Postgres `LISTEN/NOTIFY`.

use tokio::sync::broadcast::error::RecvError;
use vantage_core::{Result, error};
use vantage_types::Entity;
use vantage_vista::{VistaChange, VistaChangeStream};

use super::PostgresTableShell;
use crate::postgres::notify::HubEvent;
use crate::postgres::types::AnyPostgresType;

impl<E> PostgresTableShell<E>
where
    E: Entity<AnyPostgresType> + 'static,
{
    /// Listen on the shell's channels (its own `{table}_changed`, the
    /// `listen:` tables' channels) and stream a coarse
    /// [`VistaChange::Invalidated`] on every notification.
    ///
    /// Postgres notifications carry no row payload, so this is the
    /// invalidate-and-reconcile end of the spectrum: the consumer re-reads
    /// the set on each signal. The application installs a trigger that
    /// `pg_notify`s the channel on every write.
    ///
    /// `LISTEN` succeeds whether or not that trigger exists, and an
    /// un-triggered channel never fires, so a channel is listened on only
    /// when declared: via
    /// [`PostgresVistaFactory::with_notify`](crate::postgres::vista::PostgresVistaFactory::with_notify)
    /// or the spec's `postgres.listen`.
    ///
    /// Every stream of a data source shares its one listener connection
    /// (see `PostgresDB::notify_hub`), so open lists don't hold a pool
    /// connection each.
    pub(super) fn listen(&self) -> Result<VistaChangeStream> {
        let channels = self.extra.names().to_vec();
        let db = self.table.data_source().clone();

        let stream = async_stream::try_stream! {
            let mut feed = db
                .notify_hub()
                .subscribe(channels.clone())
                .await
                .map_err(|e| error!("LISTEN failed", channels = channels.join(", "), details = e.to_string()))?;
            loop {
                match feed.recv().await {
                    Ok(HubEvent::Notified(channel)) => {
                        if channels.iter().any(|c| **c == *channel) {
                            yield VistaChange::Invalidated;
                        }
                    }
                    Ok(HubEvent::Failed(message)) => {
                        Err(error!("notification listener failed", details = message.to_string()))?;
                    }
                    // Missed notifications still mean the set changed.
                    Err(RecvError::Lagged(_)) => yield VistaChange::Invalidated,
                    Err(RecvError::Closed) => {
                        Err(error!("notification listener stopped"))?;
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}
