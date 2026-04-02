use surreal_client::SurrealConnection;
use vantage_config::VantageConfig;
use vantage_core::{error, util::error::Context, Result};
use vantage_surrealdb::SurrealDB;
use vantage_table::{EmptyEntity, Table};

pub use vantage_config;
pub use vantage_surrealdb;

/// Get a dynamic table from config
pub fn get_table(
    config: &VantageConfig,
    entity_name: &str,
    db: SurrealDB,
) -> Result<Table<SurrealDB, EmptyEntity>> {
    config
        .get_table(entity_name, db)
        .ok_or_else(|| error!("Entity not found in config", entity_name = entity_name))
}

/// Connect to SurrealDB using DSN from environment or default
pub async fn connect_surrealdb() -> Result<SurrealDB> {
    connect_surrealdb_with_debug(false).await
}

/// Connect to SurrealDB with optional debug mode
pub async fn connect_surrealdb_with_debug(debug: bool) -> Result<SurrealDB> {
    let dsn = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "ws://root:root@localhost:8000/bakery/v2".to_string());

    let client = SurrealConnection::dsn(&dsn)
        .with_context(|| error!("Failed to parse DSN", dsn = &dsn))?
        .with_debug(debug)
        .connect()
        .await
        .with_context(|| error!("Failed to connect to SurrealDB", dsn = &dsn))?;

    if debug {
        println!("🔧 Debug mode enabled - queries will be logged");
    }

    Ok(SurrealDB::new(client))
}
