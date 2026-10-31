//! [`MongoTx`]: one MongoDB datasource's share of a
//! [`vantage_vista::Transaction`], a [`TxSlot`] over one session's
//! transaction, started on first use.
//!
//! A write to a document another transaction holds fails at once with a
//! `WriteConflict` (labelled `TransientTransactionError`). That error comes
//! back from the write like any other and fails the commit; nothing here
//! retries it.

use async_trait::async_trait;
use mongodb::{Client, ClientSession};
use vantage_core::{Result, error};
use vantage_vista::{TxBackend, TxSlot};

pub(crate) type MongoTx = TxSlot<MongoBegin>;

/// Runs the driver action `$op` in `$db`'s transaction when it has one,
/// marking the transaction failed unless it returns `Ok`.
macro_rules! in_session {
    ($db:expr, $op:expr) => {
        match $db.tx() {
            None => $op.await,
            Some(tx) => {
                let mut session = tx.open().await?;
                tx.watch($op.session(&mut *session)).await
            }
        }
    };
}

/// [`in_session!`] for an action that opens a cursor: every document it
/// yields, collected.
macro_rules! collect_in_session {
    ($db:expr, $op:expr) => {
        match $db.tx() {
            None => match $op.await {
                Ok(cursor) => cursor.try_collect().await,
                Err(e) => Err(e),
            },
            Some(tx) => {
                let mut session = tx.open().await?;
                tx.watch(async {
                    let mut cursor = $op.session(&mut *session).await?;
                    cursor.stream(&mut *session).try_collect().await
                })
                .await
            }
        }
    };
}

pub(crate) use {collect_in_session, in_session};

pub(crate) struct MongoBegin {
    client: Client,
}

impl MongoBegin {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl TxBackend for MongoBegin {
    type Open = ClientSession;

    async fn begin(&self) -> Result<ClientSession> {
        let begin_failed = |e: mongodb::error::Error| {
            error!("couldn't begin the transaction", details = e.to_string())
        };
        let mut session = self.client.start_session().await.map_err(begin_failed)?;
        session.start_transaction().await.map_err(begin_failed)?;
        Ok(session)
    }

    async fn commit(&self, mut session: ClientSession) -> Result<()> {
        session
            .commit_transaction()
            .await
            .map_err(|e| error!("transaction commit failed", details = e.to_string()))
    }

    /// A failed explicit abort still aborts: the driver queues one when the
    /// session drops, and the server expires it otherwise.
    async fn rollback(&self, mut session: ClientSession) -> Result<()> {
        session
            .abort_transaction()
            .await
            .map_err(|e| error!("transaction rollback failed", details = e.to_string()))
    }
}
