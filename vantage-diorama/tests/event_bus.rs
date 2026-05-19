//! Stage 4 (residual): edge cases not covered by the cucumber suite.
//! `handle_event` without a callback is a no-op contract; the stream
//! forwarder pattern documents the canonical shape user code uses to
//! pump an external `LiveStream`/`mpsc`/`broadcast` into a Dio.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{ChangeEvent, Lens};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    Vista::new("items", Box::new(MockShell::new().with_metadata(metadata)))
}

fn record(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r
}

#[tokio::test]
async fn handle_event_without_callback_is_noop() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            // No on_event registered.
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(master()).await?;

    // Should succeed silently — nothing to dispatch to.
    dio.handle_event(ChangeEvent::Invalidated).await?;
    Ok(())
}

#[tokio::test]
async fn spawn_forwarder_pumps_stream_into_dio() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_event(|dio, evt| {
                let dio = dio.clone();
                async move {
                    if let ChangeEvent::Updated {
                        id,
                        new: Some(record),
                    } = evt
                    {
                        dio.patched(id, record).await?;
                    }
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(master()).await?;

    // Mock "live stream" — an mpsc channel that some upstream source
    // would push into. User spawns a forwarder.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<ChangeEvent>(8);
    let dio_for_task = dio.clone();
    tokio::spawn(async move {
        while let Some(evt) = rx.recv().await {
            let _ = dio_for_task.handle_event(evt).await;
        }
    });

    tx.send(ChangeEvent::Updated {
        id: "f1".to_string(),
        new: Some(record("fwd-one")),
    })
    .await
    .unwrap();
    tx.send(ChangeEvent::Updated {
        id: "f2".to_string(),
        new: Some(record("fwd-two")),
    })
    .await
    .unwrap();

    // Forwarder task pumps the mpsc, on_event fires, cache writes —
    // poll until both rows land or we time out (CI runners are slow).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if dio.cache().list_values().await?.len() == 2 {
            break;
        }
        if std::time::Instant::now() >= deadline {
            panic!(
                "forwarder + on_event did not populate cache within 5s (have {} rows)",
                dio.cache().list_values().await?.len()
            );
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}
