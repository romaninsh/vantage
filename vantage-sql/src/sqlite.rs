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

use sqlx::sqlite::SqlitePool;
use vantage_vista::{Transaction, TxKey};

use crate::sql_tx::SqlTx;

pub use types::{AnySqliteType, SqliteType};

crate::define_typed_ident!(
    SqliteIdent,
    sqlite_ident,
    AnySqliteType,
    crate::condition::SqliteCondition
);

/// SQLite provider. Wraps a connection pool, or one transaction on it.
#[derive(Clone)]
pub struct SqliteDB {
    pool: SqlitePool,
    /// Identity shared by every clone: one transaction per datasource.
    key: Arc<()>,
    tx: Option<Arc<SqlTx<sqlx::Sqlite>>>,
}

impl SqliteDB {
    /// Create from an existing sqlx connection pool.
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            key: Arc::new(()),
            tx: None,
        }
    }

    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        Ok(Self::new(SqlitePool::connect(url).await?))
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// This datasource running its statements on `tx`. `BEGIN IMMEDIATE`
    /// takes the write lock up front, so two writers queue instead of
    /// failing to upgrade a read lock.
    pub fn in_transaction(&self, tx: &Transaction) -> vantage_core::Result<Self> {
        tx.check_open()?;
        let joined = tx.participant(self.tx_key(), || {
            SqlTx::new(self.pool.clone(), "BEGIN IMMEDIATE", tx)
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
        select: &statements::SqliteSelect,
        func: &str,
        column: impl vantage_expressions::Expressive<AnySqliteType>,
    ) -> vantage_core::Result<AnySqliteType> {
        use vantage_expressions::ExprDataSource;
        let expr = select.as_aggregate(func, column);
        let result = self.execute(&expr).await?;
        Ok(result.unwrap_scalar())
    }
}
