//! The transaction checks over a `MockShell` on a datasource without
//! transactions: its rebind is `direct`, so only the direct-write probes run.

use async_trait::async_trait;
use vantage_core::Result;
use vantage_vista::contract::{self, TxFixture, TxTables};
use vantage_vista::{
    Column, ColumnFlag, TableShell, Transaction, Vista, VistaMetadata, direct, forward_table_shell,
    mocks::MockShell,
};

#[derive(Clone)]
struct Untransacted {
    inner: Box<dyn TableShell>,
}

forward_table_shell!(Untransacted, inner, {
    fn rebind(&self, _tx: &Transaction) -> Result<Box<dyn TableShell>> {
        Ok(direct(Box::new(self.clone())))
    }
});

fn table(name: &str) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
        .with_column(Column::new("n", "i64"))
        .with_id_column("id");
    Vista::new(name, MockShell::new().with_metadata(metadata))
        .map_shell(|inner| Untransacted { inner })
}

struct DirectTx;

#[async_trait]
impl TxFixture for DirectTx {
    async fn fresh(&self) -> TxTables {
        TxTables::new(table("a"), table("b"))
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
async fn mock_direct_transactions_hold_the_contract() {
    contract::check_transactions(&DirectTx).await
}

#[cfg(feature = "rhai")]
#[tokio::test]
async fn mock_direct_atomic_holds_the_contract() {
    contract::check_atomic_rhai(&DirectTx).await
}
