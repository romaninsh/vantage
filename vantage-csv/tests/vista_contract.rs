//! `vantage_vista::contract` over the CSV files in `data/`. The backend is
//! read-only, so there are no write probes.

#![cfg(feature = "vista")]

use async_trait::async_trait;
use ciborium::Value as CborValue;
use vantage_csv::{AnyCsvType, Csv};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::contract::{TxFixture, TxTables};
use vantage_vista::{Vista, contract};

fn csv() -> Csv {
    Csv::new(format!("{}/data", env!("CARGO_MANIFEST_DIR")))
}

/// Three clients, two of them paying, with a `bakery` has-one reference.
fn clients() -> Vista {
    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("client", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<bool>("is_paying_client")
        .with_one("bakery", "bakery_id", |csv: Csv| {
            Table::<Csv, Record<AnyCsvType>>::new("bakery", csv)
                .with_id_column("id")
                .with_column_of::<String>("name")
        });
    csv.vista_factory().from_table(table).unwrap()
}

#[tokio::test]
async fn csv_contract_advertised() {
    contract::check_advertised(&clients()).await;
}

#[tokio::test]
async fn csv_contract_clone() {
    contract::check_clone(&clients(), "is_paying_client", CborValue::Bool(true)).await;
}

fn bakeries() -> Vista {
    let csv = csv();
    let table = Table::<Csv, EmptyEntity>::new("bakery", csv.clone())
        .with_id_column("id")
        .with_column_of::<String>("name");
    csv.vista_factory().from_table(table).unwrap()
}

/// Files have no transactions, and the backend takes no writes.
struct Files;

#[async_trait]
impl TxFixture for Files {
    async fn fresh(&self) -> TxTables {
        TxTables::new(clients(), bakeries())
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
async fn csv_transactions_hold_the_contract() {
    contract::check_transactions(&Files).await
}
