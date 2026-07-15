//! Stage 4 demo: external "live stream" (mocked as an mpsc channel)
//! pushes ChangeEvents into the Dio via `handle_event`. The `on_event`
//! callback mirrors the new value into the cache via `dio.patched`,
//! which publishes a `DioEvent::RecordChanged` for any subscriber.
//!
//! Run with:
//!   cargo run -p vantage-diorama --example live_invalidation

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{ChangeEvent, DioEvent, Lens};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    Vista::new("rooms", Box::new(MockShell::new().with_metadata(metadata)))
}

fn record(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r
}

#[tokio::main]
async fn main() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");

    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_event(|dio, evt| {
                let dio = dio.clone();
                async move {
                    match evt {
                        ChangeEvent::Updated {
                            id,
                            new: Some(record),
                        }
                        | ChangeEvent::Inserted {
                            id,
                            new: Some(record),
                        } => {
                            println!("on_event: mirroring {id} into cache");
                            dio.patched(id, record).await?;
                        }
                        ChangeEvent::Deleted { id } => {
                            println!("on_event: deleting {id} from cache");
                            dio.cache().delete_value(&id).await?;
                            dio.notify_record_changed(id);
                        }
                        ChangeEvent::Invalidated => {
                            println!("on_event: full invalidation");
                            dio.cache().clear().await?;
                            dio.notify_dataset_changed();
                        }
                        _ => {}
                    }
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    );

    let dio = lens.make_dio(master()).await?;
    let mut bus = dio.subscribe_events();

    // Background watcher — what a Scenery would do later.
    tokio::spawn(async move {
        while let Ok(evt) = bus.recv().await {
            match evt {
                DioEvent::RecordChanged { id } => {
                    println!("  bus: RecordChanged {id}");
                }
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
            if let Err(e) = dio_for_task.handle_event(evt).await {
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

    // Let the forwarder + on_event drain.
    tokio::time::sleep(Duration::from_millis(50)).await;

    println!("\nfinal cache:");
    for (id, _) in dio.vista().list_values().await? {
        println!("  {id}");
    }
    Ok(())
}
