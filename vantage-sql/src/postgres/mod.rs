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
use sqlx::Connection as _;
use sqlx::postgres::{PgConnection, PgPool, PgPoolOptions};

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
    /// Fails at once when the server refuses or is unreachable. The
    /// pool's own `connect` would retry a failed handshake with backoff
    /// until its 30-second acquire deadline, so one probe connection
    /// goes first and the pool is built lazily behind it.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        PgConnection::connect(url).await?.close().await?;
        let pool = PgPoolOptions::new().connect_lazy(url)?;
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
