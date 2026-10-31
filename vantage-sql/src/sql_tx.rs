//! [`SqlTx`]: one SQL datasource's share of a [`vantage_vista::Transaction`],
//! a [`TxSlot`] over one sqlx transaction on the pool, begun on first use
//! with the dialect's `BEGIN` statement.

use async_trait::async_trait;
use sqlx::{Database, Pool};
use vantage_core::{Result, error};
use vantage_vista::{TxBackend, TxSlot};

pub(crate) type SqlTx<DB> = TxSlot<SqlBegin<DB>>;

pub(crate) struct SqlBegin<DB: Database> {
    pool: Pool<DB>,
    begin: &'static str,
}

impl<DB: Database> SqlBegin<DB> {
    pub(crate) fn new(pool: Pool<DB>, begin: &'static str) -> Self {
        Self { pool, begin }
    }
}

#[async_trait]
impl<DB: Database> TxBackend for SqlBegin<DB> {
    type Open = sqlx::Transaction<'static, DB>;

    async fn begin(&self) -> Result<Self::Open> {
        self.pool
            .begin_with(self.begin)
            .await
            .map_err(|e| error!("couldn't begin the transaction", details = e.to_string()))
    }

    async fn commit(&self, open: Self::Open) -> Result<()> {
        open.commit()
            .await
            .map_err(|e| error!("transaction commit failed", details = e.to_string()))
    }

    /// A failed explicit rollback still rolls back: sqlx queues one when the
    /// transaction drops.
    async fn rollback(&self, open: Self::Open) -> Result<()> {
        open.rollback()
            .await
            .map_err(|e| error!("transaction rollback failed", details = e.to_string()))
    }
}
