use std::future::pending;
use std::time::Duration;

use async_trait::async_trait;
use tokio::time::timeout;
use vantage_core::{Result, VantageError};
use vantage_vista::{Transaction, TxBackend, TxParticipant, TxSlot};

struct Noop;

#[async_trait]
impl TxBackend for Noop {
    type Open = ();

    async fn begin(&self) -> Result<()> {
        Ok(())
    }
    async fn commit(&self, _: ()) -> Result<()> {
        Ok(())
    }
    async fn rollback(&self, _: ()) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn dropped_operation_fails_the_commit() {
    let tx = Transaction::new();
    let ok = TxSlot::new(Noop, &tx);
    ok.watch(async { Ok::<_, VantageError>(()) }).await.unwrap();
    ok.commit().await.unwrap();

    let dropped = TxSlot::new(Noop, &tx);
    let cancelled = timeout(
        Duration::from_millis(10),
        dropped.watch(pending::<Result<()>>()),
    );
    assert!(cancelled.await.is_err());
    let err = dropped.commit().await.unwrap_err();
    assert!(
        err.to_string()
            .contains("a statement failed inside the transaction; rolled back"),
        "{err}"
    );
}
