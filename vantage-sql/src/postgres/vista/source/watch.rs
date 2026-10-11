//! Change feed over Postgres `LISTEN/NOTIFY`.

use vantage_core::{Result, error};
use vantage_types::Entity;
use vantage_vista::{VistaChange, VistaChangeStream};

use super::PostgresTableShell;
use crate::postgres::types::AnyPostgresType;

impl<E> PostgresTableShell<E>
where
    E: Entity<AnyPostgresType> + 'static,
{
    /// Listen on `{table}_changed` and stream a coarse
    /// [`VistaChange::Invalidated`] on every notification.
    ///
    /// Postgres notifications carry no row payload, so this is the
    /// invalidate-and-reconcile end of the spectrum: the consumer re-reads
    /// the set on each signal. The application installs a trigger that
    /// `pg_notify`s the channel on every write.
    ///
    /// `LISTEN` succeeds whether or not that trigger exists, and an
    /// un-triggered channel never fires, so `Subscribe` is opt-in via
    /// [`PostgresVistaFactory::with_notify`](crate::postgres::vista::PostgresVistaFactory::with_notify),
    /// not inferred from the table being writable.
    pub(super) fn listen(&self) -> Result<VistaChangeStream> {
        let channel = format!("{}_changed", self.table.table_name());
        let pool = self.table.data_source().pool().clone();

        let stream = async_stream::try_stream! {
            let mut listener = sqlx::postgres::PgListener::connect_with(&pool)
                .await
                .map_err(|e| error!("open pg listener", details = e.to_string()))?;
            listener
                .listen(&channel)
                .await
                .map_err(|e| error!("LISTEN failed", channel = channel.clone(), details = e.to_string()))?;
            loop {
                listener
                    .recv()
                    .await
                    .map_err(|e| error!("recv notification", details = e.to_string()))?;
                yield VistaChange::Invalidated;
            }
        };
        Ok(Box::pin(stream))
    }
}
