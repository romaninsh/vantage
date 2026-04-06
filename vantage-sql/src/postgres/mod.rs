#[macro_use]
mod macros;
pub mod impls;
mod operation;
mod row;
pub mod statements;
pub mod types;

use sqlx::postgres::PgPool;

pub use types::{AnyPostgresType, PostgresType};

/// PostgreSQL provider. Wraps a connection pool.
#[derive(Clone)]
pub struct PostgresDB {
    pool: PgPool,
}

impl PostgresDB {
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPool::connect(url).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
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
        Ok(match result.value() {
            serde_json::Value::Array(arr) => arr
                .first()
                .and_then(|row| row.as_object())
                .and_then(|obj| obj.values().next())
                .map(|v| AnyPostgresType::untyped(v.clone()))
                .unwrap_or(result),
            _ => result,
        })
    }
}
