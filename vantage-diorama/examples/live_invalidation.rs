//! Live-ingress demo: an external "live stream" (mocked as an mpsc channel)
//! pushes ChangeEvents into the Dio via `apply_change`, which writes each
//! change into the cache and publishes the matching `DioEvent` for any
//! subscriber.
//!
//! Run with:
//!   cargo run -p vantage-diorama --example live_invalidation

use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, ChangeEvent, DioEvent, Lens};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    Vista::new("rooms", MockShell::new().with_metadata(metadata))
}

fn record(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r
}

#[tokio::main]
async fn main() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");

    let lens = Lens::builder()
        .cache(Cache::redb(tmp.path().join("cache.redb")))
        .build()?;

    let dio = lens.dio(master()).build().await?;
    let mut bus = dio.subscribe_events();

    // Background watcher — what a Scenery would do later.
    tokio::spawn(async move {
        while let Ok(evt) = bus.recv().await {
            match evt {
                DioEvent::RecordInserted { id } => println!("  bus: RecordInserted {id}"),
                DioEvent::RecordChanged { id } => println!("  bus: RecordChanged {id}"),
                DioEvent::RecordRemoved { id } => println!("  bus: RecordRemoved {id}"),
                DioEvent::DatasetChanged => println!("  bus: DatasetChanged"),
                _ => {}
            }
        }
    });

    // Mock live stream — what a SurrealDB LIVE adapter would feed.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<ChangeEvent>(8);
    let dio_for_task = dio.clone();
    tokio::spawn(async move {
        while let Some(evt) = rx.recv().await {
            if let Err(e) = dio_for_task.apply_change(evt).await {
                eprintln!("forwarder error: {e}");
            }
        }
    });

    tx.send(ChangeEvent::Inserted {
        id: "r1".to_string(),
        new: Some(record("Lobby")),
    })
    .await
    .unwrap();
    tx.send(ChangeEvent::Updated {
        id: "r1".to_string(),
        new: Some(record("Lobby (renamed)")),
    })
    .await
    .unwrap();
    tx.send(ChangeEvent::Deleted {
        id: "r1".to_string(),
    })
    .await
    .unwrap();

    // Let the forwarder drain.
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!("\nfinal cache:");
    for (id, _) in dio.cached().list_values().await? {
        println!("  {id}");
    }
    Ok(())
}
