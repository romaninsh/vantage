#[macro_use]
mod macros;
pub mod impls;
mod operation;
pub(crate) mod row;
pub mod statements;
pub mod types;

use sqlx::sqlite::SqlitePool;

pub use types::{AnySqliteType, SqliteType};

/// SQLite provider. Wraps a connection pool.
#[derive(Clone)]
pub struct SqliteDB {
    pool: SqlitePool,
}

impl SqliteDB {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(url).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
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
