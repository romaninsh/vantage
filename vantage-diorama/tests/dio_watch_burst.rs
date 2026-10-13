//! Live follow under a burst of pushed changes.
//!
//! - A burst larger than a Dio's event bus still reaches a narrowed child
//!   Dio: the follow applies it in runs, and a child that lags behind them
//!   re-reads the set and catches up.
//! - A narrowed master's stream is not trusted to filter itself: a derived
//!   live Dio holds every pushed row against its own query.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use futures::StreamExt;
use tokio::sync::Semaphore;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Augmentation, Cache, Detail, Dio, Fetch, Lens, MergeRule, Source};
use vantage_types::Record;
use vantage_vista::{
    Capability, Column, Condition, FilterOp, TableShell, Vista, VistaCapabilities, VistaChange,
    VistaChangeStream, VistaMetadata, VistaQuery, forward_table_shell, mocks::MockShell,
};

const BURST: usize = 200;

/// Rows pushed that are "In transit"; every fourth is "Delivered".
const IN_TRANSIT: usize = BURST - BURST / 4;

fn shipment(status: &str) -> Record<CborValue> {
    [("status".to_string(), CborValue::Text(status.into()))]
        .into_iter()
        .collect()
}

/// A watchable master whose stream yields `BURST` inserts at once, then
/// stays open. The rows are stored in `store` as the burst is pushed, as a
/// backend holds what it reports. The stream ignores the master's narrowing:
/// it carries rows on both sides of any status condition. With a `gate`, the
/// burst waits for one permit.
#[derive(Clone)]
struct BurstShell {
    inner: Box<dyn TableShell>,
    store: MockShell,
    capabilities: VistaCapabilities,
    gate: Option<Arc<Semaphore>>,
}

/// The `i`th row of the burst: every fourth is "Delivered".
fn burst_row(i: usize) -> (String, Record<CborValue>) {
    let status = if i.is_multiple_of(4) {
        "Delivered"
    } else {
        "In transit"
    };
    (format!("s{i:03}"), shipment(status))
}

forward_table_shell!(BurstShell, inner, {
    async fn watch(&self) -> Result<VistaChangeStream> {
        let burst = (0..BURST).map(|i| {
            let (id, value) = burst_row(i);
            Ok(VistaChange::Inserted { id, value })
        });
        let (gate, store) = (self.gate.clone(), self.store.clone());
        let opened = futures::stream::once(async move {
            if let Some(gate) = gate {
                gate.acquire().await.expect("gate open").forget();
            }
            for (id, row) in (0..BURST).map(burst_row) {
                store.set_record(id, row);
            }
        });
        Ok(Box::pin(
            opened
                .flat_map(move |()| futures::stream::iter(burst.clone()))
                .chain(futures::stream::pending()),
        ))
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
});

fn master(gate: Option<Arc<Semaphore>>) -> Vista {
    let inner = MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("status", "String"))
            .with_id_column("id"),
    );
    let capabilities = inner.capabilities().with(Capability::Subscribe);
    let store = inner.clone();
    Vista::new("shipment", inner).map_shell(|inner| BurstShell {
        inner,
        store,
        capabilities,
        gate,
    })
}

async fn live_dio(master: Vista) -> Result<Dio> {
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    lens.dio(master).live().build().await
}

/// Wait until `dio`'s cache holds `want` rows, bounded so a hang fails.
async fn wait_for_count(dio: &Dio, want: i64) -> bool {
    tokio::time::timeout(Duration::from_secs(5), async {
        while dio.cached().count().await? < want {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Ok::<_, vantage_core::VantageError>(())
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn a_narrowed_child_follows_a_parent_burst_larger_than_the_bus() -> Result<()> {
    let gate = Arc::new(Semaphore::new(0));
    let parent = live_dio(master(Some(gate.clone()))).await?;
    let in_transit = parent
        .cached()
        .with_condition("status", FilterOp::Eq, "In transit")?;
    let child = live_dio(in_transit).await?;
    // The child subscribes to the parent's bus before the burst lands.
    gate.add_permits(1);

    assert!(
        wait_for_count(&child, IN_TRANSIT as i64).await,
        "child holds {} of {IN_TRANSIT} rows",
        child.cached().count().await?
    );
    assert!(wait_for_count(&parent, BURST as i64).await);
    Ok(())
}

#[tokio::test]
async fn a_derived_live_dio_filters_an_unfiltered_stream() -> Result<()> {
    let dio = live_dio(master(None)).await?;
    let in_transit = dio
        .with_query(&VistaQuery::new().with_condition(Condition::new(
            "status",
            FilterOp::Eq,
            "In transit",
        )))
        .await?;
    assert!(
        in_transit.master().query().conditions.len() == 1,
        "the master took the condition, and its stream still pushes every row"
    );

    assert!(wait_for_count(&in_transit, IN_TRANSIT as i64).await);
    // Give the rest of the burst time to land, then check nothing outside
    // the condition did.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let rows = in_transit.cached().list_values().await?;
    assert_eq!(rows.len(), IN_TRANSIT);
    assert!(
        rows.values()
            .all(|row| row.get("status") == Some(&CborValue::Text("In transit".into()))),
        "a Delivered row reached the narrowed cache"
    );
    // The parent follows its own, unnarrowed stream.
    assert!(wait_for_count(&dio, BURST as i64).await);
    Ok(())
}

/// The follow of an augmented Dio still holds pushed rows against a search
/// its master accepted: the master searched its own columns, which a pushed
/// row carries.
#[tokio::test]
async fn an_augmented_live_dio_filters_by_an_accepted_search() -> Result<()> {
    let notes = Vista::new(
        "notes",
        MockShell::new().with_metadata(
            VistaMetadata::new()
                .with_column(Column::new("id", "String").with_flag("id"))
                .with_column(Column::new("note", "String"))
                .with_id_column("id"),
        ),
    );
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    let dio = lens
        .dio(master(None))
        .augment(vec![Augmentation {
            detail: Detail::Fixed(Arc::new(notes)),
            source: Source::Id,
            fetch: Fetch::PerRow,
            merge: MergeRule {
                columns: vec!["note".into()],
            },
        }])
        .live()
        .build()
        .await?;
    let in_transit = dio
        .with_query(&VistaQuery::new().with_search("transit"))
        .await?;
    assert!(
        in_transit.master().capabilities().has(Capability::Search)
            && in_transit.master().query().search.is_some(),
        "the master takes the search"
    );

    assert!(wait_for_count(&in_transit, IN_TRANSIT as i64).await);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let rows = in_transit.cached().list_values().await?;
    assert_eq!(rows.len(), IN_TRANSIT, "a Delivered row reached the cache");
    Ok(())
}
