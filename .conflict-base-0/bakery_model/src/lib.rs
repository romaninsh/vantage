use std::sync::OnceLock;
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use tokio_postgres::NoTls;

use vantage::prelude::Postgres;

pub mod bakery;
pub use bakery::*;

pub mod client;
pub use client::*;

pub mod product;
pub use product::*;

pub mod lineitem;
pub use lineitem::*;

pub mod order;
pub use order::*;

static POSTGRESS: OnceLock<Postgres> = OnceLock::new();

pub fn set_postgres(postgres: Postgres) -> Result<()> {
    POSTGRESS
        .set(postgres)
        .map_err(|e| anyhow::anyhow!("Failed to set Postgres instance: {:?}", e))
}

pub fn postgres() -> Postgres {
    POSTGRESS
        .get()
        .expect("Postgres has not been initialized. use connect_postgress()")
        .clone()
}

pub async fn connect_postgres() -> Result<()> {
    // If already connected, just return success
    if POSTGRESS.get().is_some() {
        return Ok(());
    }

    let connection_string = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@localhost:5432/postgres".to_string());

    let postgres = Postgres::new(&connection_string).await;
    set_postgres(postgres)
}
