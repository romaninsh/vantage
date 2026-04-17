use std::sync::OnceLock;

use vantage_mongodb::prelude::*;

static DB: OnceLock<MongoDB> = OnceLock::new();

pub async fn init(url: &str, database: &str) -> VantageResult<()> {
    let conn = MongoDB::connect(url, database)
        .await
        .context("Failed to connect to MongoDB")?;
    DB.set(conn).ok();
    Ok(())
}

pub fn db() -> MongoDB {
    DB.get().expect("database not initialised").clone()
}
