#[macro_use]
mod macros;
mod row;
pub mod impls;
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
}
