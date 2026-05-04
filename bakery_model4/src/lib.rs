//! YAML-driven bakery models for SQLite via Vista.
//!
//! Each entity lives in its own YAML file under `bakery_model4/vistas/`.
//! The factory parses the spec and materializes a `Vista` bound to the
//! caller-supplied `SqliteDB`.

use vantage_core::{Result, util::error::Context};
use vantage_sql::sqlite::SqliteDB;
pub use vantage_sql::sqlite::SqliteDB as Db;
pub use vantage_vista::Vista;

const BAKERY_YAML: &str = include_str!("../vistas/bakery.yaml");
const CLIENT_YAML: &str = include_str!("../vistas/client.yaml");
const PRODUCT_YAML: &str = include_str!("../vistas/product.yaml");
const ORDER_YAML: &str = include_str!("../vistas/order.yaml");

/// Names of every vista this crate exposes, in CLI display order.
pub fn entity_names() -> &'static [&'static str] {
    &["bakery", "client", "product", "order"]
}

/// Resolve an entity name to its bundled YAML.
pub fn entity_yaml(entity: &str) -> Option<&'static str> {
    match entity {
        "bakery" => Some(BAKERY_YAML),
        "client" => Some(CLIENT_YAML),
        "product" => Some(PRODUCT_YAML),
        "order" => Some(ORDER_YAML),
        _ => None,
    }
}

/// Build a Vista for the named entity by parsing its bundled YAML.
pub fn vista(db: SqliteDB, entity: &str) -> Result<Vista> {
    use vantage_vista::VistaFactory;

    let yaml = entity_yaml(entity).ok_or_else(|| {
        vantage_core::error!("Unknown entity in bakery_model4", entity = entity)
    })?;
    db.vista_factory()
        .from_yaml(yaml)
        .with_context(|| vantage_core::error!("Failed to build vista from YAML", entity = entity))
}

/// Connect to SQLite using DSN from environment or default.
pub async fn connect_sqlite() -> Result<SqliteDB> {
    let dsn =
        std::env::var("SQLITE_URL").unwrap_or_else(|_| "sqlite:target/bakery.sqlite".to_string());
    SqliteDB::connect(&dsn)
        .await
        .map_err(|e| vantage_core::error!("Failed to connect to SQLite", details = e.to_string()))
}
