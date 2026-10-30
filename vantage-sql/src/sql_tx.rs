//! [`SqlTx`]: one SQL datasource's share of a [`vantage_vista::Transaction`].
//! Every clone of the datasource in transaction mode holds the same `SqlTx`,
//! so their statements run on one sqlx transaction, begun on first use.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use sqlx::{Database, Pool};
use tokio::sync::{Mutex, MutexGuard};
use vantage_core::{Result, VantageError, error};
use vantage_vista::{Transaction, TxParticipant, WeakTransaction};

const FAILED_STATEMENT: &str = "a statement failed inside the transaction; rolled back";

pub(crate) struct SqlTx<DB: Database> {
    pool: Pool<DB>,
    begin: &'static str,
    slot: Mutex<Option<sqlx::Transaction<'static, DB>>>,
    failed: AtomicBool,
    /// Set under the slot lock by commit or rollback.
    closed: AtomicBool,
    // Weak: the transaction holds this participant, so a strong handle back
    // would keep both alive and the sqlx transaction would never roll back
    // on drop.
    owner: WeakTransaction,
}

impl<DB: Database> SqlTx<DB> {
    pub(crate) fn new(pool: Pool<DB>, begin: &'static str, owner: &Transaction) -> Self {
        Self {
            pool,
            begin,
            slot: Mutex::new(None),
            failed: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            owner: owner.downgrade(),
        }
    }

    pub(crate) fn transaction(&self) -> Option<Transaction> {
        self.owner.upgrade()
    }

    /// The open transaction's connection, begun now if nothing has run on it
    /// yet. Held until the guard drops, so statements on it run one at a
    /// time.
    pub(crate) async fn connection(&self) -> Result<Open<'_, DB>> {
        match self.transaction() {
            Some(tx) if !tx.is_finished() => {}
            Some(_) => return Err(finished()),
            None => {
                // The transaction was dropped unfinished, so no commit can be
                // running: drop what began now, which rolls it back and frees
                // the connection, instead of when the last clone goes.
                if let Ok(mut slot) = self.slot.try_lock() {
                    slot.take();
                }
                return Err(finished());
            }
        }
        let mut slot = self.slot.lock().await;
        // Commit or rollback may have run between the check above and the
        // lock; beginning again would open a transaction nobody finishes.
        if self.closed.load(Ordering::SeqCst) {
            return Err(finished());
        }
        if slot.is_none() {
            let begun = self.pool.begin_with(self.begin).await.map_err(|e| {
                self.failed.store(true, Ordering::SeqCst);
                error!("couldn't begin the transaction", details = e.to_string())
            })?;
            *slot = Some(begun);
        }
        Ok(Open(slot))
    }

    /// Runs `statement`, marking the transaction failed unless it returns
    /// `Ok`: commit then rolls back. SQLite and MySQL keep a transaction open
    /// after a failed statement, so the server alone wouldn't refuse the
    /// commit. A statement dropped mid-flight (a timeout, a `select!`) counts
    /// as failed too: the server may still have run it.
    pub(crate) async fn watch<R>(&self, statement: impl Future<Output = Result<R>>) -> Result<R> {
        let armed = Armed(&self.failed);
        let out = statement.await;
        if out.is_ok() {
            std::mem::forget(armed);
        }
        out
    }

    /// Whatever began, taken out for good: later statements are refused.
    async fn close(&self) -> Option<sqlx::Transaction<'static, DB>> {
        let mut slot = self.slot.lock().await;
        self.closed.store(true, Ordering::SeqCst);
        slot.take()
    }
}

/// Marks the transaction failed on drop; forgotten once the statement is `Ok`.
/// It only ever sets the flag, so an earlier failure stays.
struct Armed<'a>(&'a AtomicBool);

impl Drop for Armed<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn finished() -> VantageError {
    error!("transaction finished")
}

/// The begun transaction's connection, locked. A plain `MutexGuard`, not a
/// mapped one: tokio's `MappedMutexGuard` is `Send` only under a lifetime
/// bound the compiler can't prove inside the async fns that hold it.
pub(crate) struct Open<'a, DB: Database>(MutexGuard<'a, Option<sqlx::Transaction<'static, DB>>>);

impl<DB: Database> Deref for Open<'_, DB> {
    type Target = DB::Connection;

    fn deref(&self) -> &DB::Connection {
        self.0.as_ref().expect("begun in connection()")
    }
}

impl<DB: Database> DerefMut for Open<'_, DB> {
    fn deref_mut(&mut self) -> &mut DB::Connection {
        self.0.as_mut().expect("begun in connection()")
    }
}

#[async_trait]
impl<DB: Database> TxParticipant for SqlTx<DB> {
    async fn commit(&self) -> Result<()> {
        let open = self.close().await;
        if self.failed.load(Ordering::SeqCst) {
            // A failed explicit rollback still rolls back: sqlx queues one
            // when the transaction drops.
            if let Some(open) = open
                && let Err(e) = open.rollback().await
            {
                tracing::warn!("transaction rollback failed: {e}");
            }
            return Err(error!(FAILED_STATEMENT));
        }
        let Some(open) = open else {
            return Ok(());
        };
        open.commit()
            .await
            .map_err(|e| error!("transaction commit failed", details = e.to_string()))
    }

    async fn rollback(&self) -> Result<()> {
        let Some(open) = self.close().await else {
            return Ok(());
        };
        open.rollback()
            .await
            .map_err(|e| error!("transaction rollback failed", details = e.to_string()))
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use std::future::pending;
    use std::time::Duration;

    use sqlx::SqlitePool;
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn dropped_statement_fails_the_commit() {
        let tx = Transaction::new();
        let ok = SqlTx::new(
            SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
            "BEGIN",
            &tx,
        );
        ok.watch(async { Ok(()) }).await.unwrap();
        ok.commit().await.unwrap();

        let dropped = SqlTx::new(
            SqlitePool::connect_lazy("sqlite::memory:").unwrap(),
            "BEGIN",
            &tx,
        );
        let cancelled = timeout(
            Duration::from_millis(10),
            dropped.watch(pending::<Result<()>>()),
        );
        assert!(cancelled.await.is_err());
        let err = dropped.commit().await.unwrap_err();
        assert!(err.to_string().contains(FAILED_STATEMENT));
    }
}
