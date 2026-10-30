#[macro_use]
mod macros;
pub mod impls;
pub mod operation;
pub(crate) mod row;
pub mod statements;
pub mod types;

#[cfg(feature = "vista")]
pub mod vista;

use std::sync::Arc;

use sqlx::Connection as _;
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};
use vantage_vista::{Transaction, TxKey};

use crate::sql_tx::{SqlBegin, SqlTx};

pub use types::{AnyPostgresType, PostgresType};

crate::define_typed_ident!(
    PgIdent,
    pg_ident,
    AnyPostgresType,
    crate::condition::PostgresCondition
);

/// PostgreSQL provider. Wraps a connection pool, or one transaction on it.
#[derive(Clone)]
pub struct PostgresDB {
    pool: PgPool,
    /// Identity shared by every clone: one transaction per datasource.
    key: Arc<()>,
    tx: Option<Arc<SqlTx<sqlx::Postgres>>>,
}

impl PostgresDB {
    /// Create from an existing sqlx connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            key: Arc::new(()),
            tx: None,
        }
    }

    /// Fails at once when the server refuses or is unreachable. The
    /// pool's own `connect` would retry a failed handshake with backoff
    /// until its 30-second acquire deadline, so one probe connection
    /// goes first and the pool is built lazily behind it.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        PgConnection::connect(url).await?.close().await?;
        Ok(Self::new(PgPoolOptions::new().connect_lazy(url)?))
    }

    pub fn pool(&self) -> &PgPool {
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
        select: &statements::PostgresSelect,
        func: &str,
        column: impl vantage_expressions::Expressive<AnyPostgresType>,
    ) -> vantage_core::Result<AnyPostgresType> {
        use vantage_expressions::ExprDataSource;
        let expr = select.as_aggregate(func, column);
        let result = self.execute(&expr).await?;
        Ok(crate::sql_exec::first_cell::<Self>(result))
    }
}
