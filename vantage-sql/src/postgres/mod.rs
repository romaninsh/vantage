#[macro_use]
mod macros;
pub mod impls;
pub mod operation;
pub(crate) mod row;
pub mod statements;
pub mod types;

#[cfg(feature = "vista")]
pub mod vista;

use ciborium::Value as CborValue;
use sqlx::postgres::PgPool;

pub use types::{AnyPostgresType, PostgresType};

crate::define_typed_ident!(
    PgIdent,
    pg_ident,
    AnyPostgresType,
    crate::condition::PostgresCondition
);

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
            CborValue::Array(arr) => arr
                .first()
                .and_then(|row| match row {
                    CborValue::Map(map) => map
                        .first()
                        .map(|(_, v)| AnyPostgresType::untyped(v.clone())),
                    _ => None,
                })
                .unwrap_or(result),
            _ => result,
        })
    }
}
