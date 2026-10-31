//! [`SurrealTx`]: one SurrealDB datasource's share of a
//! [`vantage_vista::Transaction`], a [`TxSlot`] over an interactive
//! transaction (`begin`/`commit`/`cancel` RPCs, SurrealDB 3+) on a connection
//! of its own, opened on first use. Every clone of a [`SurrealDB`] shares one
//! socket, and a transaction kept there would hold it for the whole block.
//!
//! SurrealDB is optimistic: two transactions writing the same record both
//! proceed, and the second to commit fails with a conflict. A failed
//! statement leaves the server's transaction open, so the slot's `failed`
//! flag is what turns it into a rollback.
//!
//! [`SurrealDB`]: super::SurrealDB

use std::sync::Arc;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use surreal_client::{SurrealClient, SurrealError};
use vantage_core::{Result, VantageError, error};
use vantage_vista::{TxBackend, TxSlot};

pub(crate) type SurrealTx = TxSlot<SurrealBegin>;

/// `TransactionConflict`: another transaction committed to the same records.
const TRANSACTION_CONFLICT: i32 = -32009;

pub(crate) struct SurrealBegin {
    shared: Arc<tokio::sync::Mutex<SurrealClient>>,
}

impl SurrealBegin {
    pub(crate) fn new(shared: Arc<tokio::sync::Mutex<SurrealClient>>) -> Self {
        Self { shared }
    }
}

/// The begun transaction: its own connection, and the id `begin` gave on it.
/// Dropping it closes the connection, which cancels the transaction.
pub(crate) struct SurrealOpen {
    pub(crate) client: SurrealClient,
    pub(crate) txn: CborValue,
}

#[async_trait]
impl TxBackend for SurrealBegin {
    type Open = SurrealOpen;

    async fn begin(&self) -> Result<SurrealOpen> {
        let begin_failed =
            |e: SurrealError| error!("couldn't begin the transaction", details = e.to_string());
        // The shared lock is held only to copy the connection settings.
        let shared = self.shared.lock().await.clone();
        let client = shared.reconnect().await.map_err(begin_failed)?;
        let txn = client.begin().await.map_err(begin_failed)?;
        Ok(SurrealOpen { client, txn })
    }

    async fn commit(&self, open: SurrealOpen) -> Result<()> {
        open.client.commit(&open.txn).await.map_err(|e| {
            marked(
                error!("transaction commit failed", details = e.to_string()),
                &e,
            )
        })
    }

    /// A failed explicit cancel still cancels: the connection closes when
    /// `open` drops.
    async fn rollback(&self, open: SurrealOpen) -> Result<()> {
        open.client
            .cancel(&open.txn)
            .await
            .map_err(|e| error!("transaction rollback failed", details = e.to_string()))
    }
}

fn marked(err: VantageError, cause: &SurrealError) -> VantageError {
    match cause {
        SurrealError::ServerError { code, .. } if *code == TRANSACTION_CONFLICT => {
            err.mark_conflict()
        }
        _ => err,
    }
}
