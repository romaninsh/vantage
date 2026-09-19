#[macro_use]
mod macros;
pub mod operation;
pub(crate) mod row;
pub mod statements;
mod table_source;
pub mod types;

#[cfg(feature = "vista")]
pub mod vista;

use ciborium::Value as CborValue;
use sqlx::Connection as _;
use sqlx::mysql::{MySqlConnection, MySqlPool, MySqlPoolOptions};

pub use types::{AnyMysqlType, MysqlType};

crate::define_typed_ident!(
    MysqlIdent,
    mysql_ident,
    AnyMysqlType,
    crate::condition::MysqlCondition
);

/// MySQL provider. Wraps a connection pool.
#[derive(Clone)]
pub struct MysqlDB {
    pool: MySqlPool,
}

impl MysqlDB {
    /// Fails at once when the server refuses or is unreachable. The
    /// pool's own `connect` would retry a failed handshake with backoff
    /// until its 30-second acquire deadline, so one probe connection
    /// goes first and the pool is built lazily behind it.
    pub async fn connect(url: &str) -> Result<Self, sqlx::Error> {
        MySqlConnection::connect(url).await?.close().await?;
        let pool = MySqlPoolOptions::new().connect_lazy(url)?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &MySqlPool {
        &self.pool
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
        Ok(match result.value() {
            CborValue::Array(arr) => arr
                .first()
                .and_then(|row| match row {
                    CborValue::Map(map) => {
                        map.first().map(|(_, v)| AnyMysqlType::untyped(v.clone()))
                    }
                    _ => None,
                })
                .unwrap_or(result),
            _ => result,
        })
    }
}

// DataSource marker trait
impl vantage_expressions::traits::datasource::DataSource for MysqlDB {}

// ExprDataSource impl
mod expr_data_source;

// SelectableDataSource impl
mod selectable_data_source;
