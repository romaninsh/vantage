//! A burst of pushed changes larger than a Dio's event bus still reaches a
//! narrowed child Dio: the watch applies it in runs its subscribers can keep
//! up with, so the child's follow never lags into an `Invalidated` it can't
//! recover from.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use futures::StreamExt;
use vantage_core::Result;
use vantage_diorama::{Dio, Lens};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, FilterOp, TableShell, Vista, VistaCapabilities, VistaChange,
    VistaChangeStream, VistaMetadata, forward_table_shell, mocks::MockShell,
};

const BURST: usize = 200;

fn shipment(status: &str) -> Record<CborValue> {
    [("status".to_string(), CborValue::Text(status.into()))]
        .into_iter()
        .collect()
}

/// A watchable master whose stream yields `BURST` inserts at once, then
/// stays open.
#[derive(Clone)]
struct BurstShell {
    inner: Box<dyn TableShell>,
    capabilities: VistaCapabilities,
}

forward_table_shell!(BurstShell, inner, {
    async fn watch(&self) -> Result<VistaChangeStream> {
        let burst = (0..BURST).map(|i| {
            Ok(VistaChange::Inserted {
                id: format!("s{i:03}"),
                value: shipment(if i % 4 == 0 {
                    "Delivered"
                } else {
                    "In transit"
                }),
            })
        });
        Ok(Box::pin(
            futures::stream::iter(burst).chain(futures::stream::pending()),
        ))
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
});

fn master() -> Vista {
    let inner = MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("status", "String"))
            .with_id_column("id"),
    );
    let capabilities = inner.capabilities().with(Capability::Subscribe);
    Vista::new("shipment", inner).map_shell(|inner| BurstShell {
        inner,
        capabilities,
    })
}

async fn dio(master: Vista) -> Result<Dio> {
    let lens = Arc::new(Lens::new().cache_in_memory().build().expect("lens"));
    lens.make_dio(master).await
}

#[tokio::test]
async fn a_narrowed_child_follows_a_parent_burst_larger_than_the_bus() -> Result<()> {
    let parent = dio(master()).await?;
    let in_transit = parent
        .vista()
        .with_condition("status", FilterOp::Eq, "In transit")?;
    let child = dio(in_transit).await?;
    child.watch().await?;
    parent.watch().await?;

    let want = (BURST - BURST / 4) as i64;
    let followed = tokio::time::timeout(Duration::from_secs(5), async {
        while child.cache().count().await? < want {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Ok::<_, vantage_core::VantageError>(())
    })
    .await;
    assert!(
        followed.is_ok(),
        "child holds {} of {want} rows",
        child.cache().count().await?
    );
    assert_eq!(parent.cache().count().await?, BURST as i64);
    Ok(())
}
