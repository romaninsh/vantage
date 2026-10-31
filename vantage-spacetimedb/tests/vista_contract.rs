//! `vantage_vista::contract` over a live `cardroom`. Ignored by default, like
//! every test that needs a host:
//!
//! ```sh
//! cargo nextest run --run-ignored all -E 'binary(vista_contract)'
//! ```
//!
//! There are no write probes: `cardroom` is shared live data, not a disposable
//! fixture.

use async_trait::async_trait;
use vantage_spacetimedb::SpacetimeDb;
use vantage_vista::contract::{TxFixture, TxTables};
use vantage_vista::{Vista, contract};

fn db() -> SpacetimeDb {
    SpacetimeDb::new(
        std::env::var("SPACETIMEDB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into()),
        std::env::var("SPACETIMEDB_DB").unwrap_or_else(|_| "cardroom".into()),
    )
}

async fn vista(relation: &str) -> Vista {
    db().vista_factory()
        .from_relation(relation)
        .await
        .expect("build vista")
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `cardroom` module published"]
async fn spacetimedb_contract_advertised() {
    contract::check_advertised(&vista("game").await).await;
}

/// `cardroom` keeps dealing, so its games are a mix of ended and in play.
#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with `cardroom` published and playing"]
async fn spacetimedb_contract_clone() {
    contract::check_clone(&vista("game").await, "status", "ended".into()).await;
}

/// The SQL API spans no transaction across statements. Without the owner's
/// token `game` takes no writes, so only the direct-read probe runs; both
/// tables are `game`, since the probe reads only the first.
struct Cardroom;

#[async_trait]
impl TxFixture for Cardroom {
    async fn fresh(&self) -> TxTables {
        TxTables::new(vista("game").await, vista("game").await)
    }

    async fn fresh_pool_of_one(&self) -> TxTables {
        self.fresh().await
    }

    fn detects_deadlocks(&self) -> bool {
        false
    }

    fn supports_transactions(&self) -> bool {
        false
    }
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `cardroom` module published"]
async fn spacetimedb_transactions_hold_the_contract() {
    contract::check_transactions(&Cardroom).await
}
