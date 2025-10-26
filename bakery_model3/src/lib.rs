use std::sync::OnceLock;
use surreal_client::SurrealConnection;
use vantage_core::{error, util::error::Context};
use vantage_dataset::dataset::Result;
pub use vantage_surrealdb::SurrealDB;
use vantage_table::prelude::*;

// Define all models in one place
models! {
    BakeryModel(SurrealDB) => {
        bakery => Bakery,
        client => Client,
        order => Order,
        product => Product,
    }
}

// with_model macro specific to BakeryModel
#[macro_export]
macro_rules! with_model {
    ($model:expr, $callback:ident, $($args:expr),*) => {
        match $model {
            BakeryModel::Bakery(table) => $callback(table, $($args),*).await,
            BakeryModel::Client(table) => $callback(table, $($args),*).await,
            BakeryModel::Order(table) => $callback(table, $($args),*).await,
            BakeryModel::Product(table) => $callback(table, $($args),*).await,
        }
    };
}

static SURREALDB: OnceLock<SurrealDB> = OnceLock::new();

pub fn set_surrealdb(db: SurrealDB) -> Result<()> {
    if SURREALDB.set(db).is_err() {
        return Err(error!("Failed to set SurrealDB instance"));
    }
    Ok(())
}

pub fn surrealdb() -> SurrealDB {
    SURREALDB
        .get()
        .expect("SurrealDB has not been initialized. use connect_surrealdb()")
        .clone()
}

pub async fn connect_surrealdb() -> Result<()> {
    connect_surrealdb_with_debug(false).await
}

pub async fn connect_surrealdb_with_debug(debug: bool) -> Result<()> {
    let dsn = std::env::var("SURREALDB_URL")
        .unwrap_or_else(|_| "ws://root:root@localhost:8000/bakery/v2".to_string());

    // if you are using this in produciton code, do not include `dns` which might
    // contain password
    let client = SurrealConnection::dsn(&dsn)
        .with_context(|| error!("Failed to parse DSN", dsn = &dsn))?
        .with_debug(debug)
        .connect()
        .await
        .with_context(|| error!("Failed to connect to SurrealDB", dsn = &dsn))?;

    let db = SurrealDB::new(client);
    set_surrealdb(db)?;

    if debug {
        println!("🔧 Debug mode enabled - queries will be logged");
    }

    Ok(())
}
