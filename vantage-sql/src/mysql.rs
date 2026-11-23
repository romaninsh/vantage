#[macro_use]
mod macros;
pub mod operation;
pub(crate) mod row;
mod set_writes;
pub mod statements;
mod table_source;
pub mod types;

#[cfg(feature = "vista")]
pub mod vista;

use std::sync::Arc;

use sqlx::Connection as _;
use sqlx::mysql::{MySqlConnection, MySqlPool, MySqlPoolOptions};
use vantage_vista::{Transaction, TxKey};

use crate::sql_tx::{SqlBegin, SqlTx};

pub use types::{AnyMysqlType, MysqlType};

crate::define_typed_ident!(
    MysqlIdent,
    mysql_ident,
    AnyMysqlType,
    crate::condition::MysqlCondition
);

/// MySQL provider. Wraps a connection pool, or one transaction on it.
#[derive(Clone)]
pub struct MysqlDB {
    pool: MySqlPool,
    /// Identity shared by every clone: one transaction per datasource.
    key: Arc<()>,
    tx: Option<Arc<SqlTx<sqlx::MySql>>>,
}

impl MysqlDB {
    /// Create from an existing sqlx connection pool. `join_concat` renders
    /// `GROUP_CONCAT`, which MySQL cuts at `group_concat_max_len` (1024 bytes
    /// by default) without an error; [`Self::connect`] raises it on every
    /// connection, a pool built elsewhere should too.
    pub fn new(pool: MySqlPool) -> Self {
        Self {
            pool,
            key: Arc::new(()),
            tx: None,
        }
    }

    /// Fails at once when the server refuses or is unreachable. The
    /// pool's own `connect` would retry a failed handshake with backoff
    /// until its 30-second acquire deadline, so one probe connection
    /// goes first and the pool is built lazily behind it. Each connection
    /// raises `group_concat_max_len`, so a `join_concat` isn't cut at 1024
    /// bytes (the packet limit still applies).
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        MySqlConnection::connect(url).await?.close().await?;
        let pool = MySqlPoolOptions::new()
            .after_connect(|conn, _| {
                Box::pin(async move {
                    sqlx::query("SET SESSION group_concat_max_len = 4294967295")
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect_lazy(url)?;
        Ok(Self::new(pool))
    }

    pub fn pool(&self) -> &MySqlPool {
        &self.pool
    }

    /// This datasource running its statements on `tx`.
    pub fn in_transaction(&self, tx: &Transaction) -> vantage_core::Result<Self> {
        tx.check_open()?;
        let joined = tx.participant(self.tx_key(), || {
            SqlTx::new(SqlBegin::new(self.pool.clone(), "BEGIN"), tx)
        })?;
        Ok(Self {
            tx: Some(joined),
            ..self.clone()
        })
    }

    pub fn tx_key(&self) -> TxKey {
        TxKey::of(&self.key)
    }

    /// Execute an aggregate query (COUNT, SUM, MAX, MIN etc.) and return the scalar result.
    pub async fn aggregate(
        &self,
        select: &statements::MysqlSelect,
        func: &str,
        column: impl vantage_expressions::Expressive<AnyMysqlType>,
    ) -> vantage_core::Result<AnyMysqlType> {
        use vantage_expressions::ExprDataSource;
        let expr = select.as_aggregate(func, column);
        let result = self.execute(&expr).await?;
        Ok(crate::sql_exec::first_cell::<Self>(result))
    }
}

impl vantage_expressions::traits::datasource::DataSource for MysqlDB {}

mod expr_data_source;

mod selectable_data_source;
