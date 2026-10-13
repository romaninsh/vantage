//! External change entering a Dio through its one ingress:
//! `apply_change` for single changes, `reconcile` for a full set.
//!
//! The stream forwarder documents the canonical shape user code uses to pump
//! an external `LiveStream`/`mpsc`/`broadcast` into a Dio.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use tokio::sync::broadcast;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, CacheBackend, ChangeEvent, Dio, DioEvent, Lens, MemoryCache};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn shell() -> MockShell {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    MockShell::new().with_metadata(metadata)
}

fn record(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r
}

async fn redb_dio(tmp: &TempDir, master: Vista) -> Result<Dio> {
    let lens = Lens::builder()
        .cache(Cache::redb(tmp.path().join("cache.redb")))
        .build()?;
    lens.dio(master).build().await
}

/// Wait for the next event `pick` accepts.
async fn next<T>(
    bus: &mut broadcast::Receiver<DioEvent>,
    pick: impl Fn(DioEvent) -> Option<T>,
) -> T {
    let wait = async {
        loop {
            if let Some(found) = bus.recv().await.ok().and_then(&pick) {
                return found;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("the event never came")
}

#[tokio::test]
async fn spawn_forwarder_pumps_stream_into_dio() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = redb_dio(&tmp, Vista::new("items", shell())).await?;
    let mut bus = dio.subscribe_events();

    // Mock "live stream" — an mpsc channel that some upstream source would
    // push into. User spawns a forwarder.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<ChangeEvent>(8);
    let dio_for_task = dio.clone();
    tokio::spawn(async move {
        while let Some(evt) = rx.recv().await {
            let _ = dio_for_task.apply_change(evt).await;
        }
    });
    for (id, name) in [("f1", "fwd-one"), ("f2", "fwd-two")] {
        let change = ChangeEvent::Inserted {
            id: id.to_string(),
            new: Some(record(name)),
        };
        tx.send(change).await.unwrap();
    }
    let inserted = |e| match e {
        DioEvent::RecordInserted { id } => Some(id),
        _ => None,
    };
    assert_eq!(next(&mut bus, inserted).await, "f1");
    assert_eq!(next(&mut bus, inserted).await, "f2");
    assert_eq!(dio.cached().list_values().await?.len(), 2);
    Ok(())
}

/// An `Invalidated` change on a Dio with no refresher re-reads the set: the
/// cache is reconciled with the master, rows it no longer holds included.
#[tokio::test]
async fn invalidated_without_a_refresher_rereads_the_set() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let master = shell();
    master.set_record("a", record("one"));
    let dio = redb_dio(&tmp, Vista::new("items", master.clone())).await?;
    dio.apply_change(ChangeEvent::Inserted {
        id: "gone".into(),
        new: Some(record("stale")),
    })
    .await?;
    let mut bus = dio.subscribe_events();

    dio.apply_change(ChangeEvent::Invalidated).await?;

    let rows = dio.cached().list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["a"]);
    let changed = |e| matches!(e, DioEvent::DatasetChanged).then_some(());
    next(&mut bus, changed).await;
    Ok(())
}

/// `reconcile` writes the snapshot, deletes cached rows absent from it, and
/// records the refresh time.
#[tokio::test]
async fn reconcile_makes_the_cache_hold_exactly_the_snapshot() -> Result<()> {
    let backend = Arc::new(MemoryCache::new());
    let lens = Lens::builder()
        .cache(Cache::backend(backend.clone()))
        .build()?;
    let dio = lens.dio(Vista::new("items", shell())).build().await?;
    let table = backend.open_table("items").await?;
    for id in ["a", "b"] {
        let change = ChangeEvent::Inserted {
            id: id.into(),
            new: Some(record("old")),
        };
        dio.apply_change(change).await?;
    }

    assert_eq!(table.meta_refreshed_at().await?, None);

    let snapshot = [
        ("b".to_string(), record("new")),
        ("c".to_string(), record("new")),
    ];
    dio.reconcile(snapshot.into_iter().collect()).await?;

    let rows = dio.cached().list_values().await?;
    assert_eq!(rows.keys().collect::<Vec<_>>(), ["b", "c"]);
    assert_eq!(rows["b"].get("name"), Some(&CborValue::Text("new".into())));
    assert!(
        table.meta_refreshed_at().await?.is_some(),
        "refresh time recorded"
    );
    Ok(())
}
