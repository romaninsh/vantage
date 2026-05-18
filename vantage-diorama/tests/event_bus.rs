//! Stage 4: event bus + on_event + convenience publishers + handle_event.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_diorama::{ChangeEvent, DioEvent, Lens};
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
async fn invalidate_record_publishes_event() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(master()).await?;
    let mut events = dio.subscribe_events();

    dio.invalidate_record("a1");

    let evt = tokio::time::timeout(Duration::from_millis(200), events.recv())
        .await
        .expect("timed out")
        .expect("bus closed");
    matches!(evt, DioEvent::RecordChanged { ref id } if id == "a1")
        .then_some(())
        .ok_or_else(|| vantage_core::error!("wrong event", got = format!("{:?}", evt)))?;
    Ok(())
}

#[tokio::test]
async fn invalidate_all_publishes_event() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(master()).await?;
    let mut events = dio.subscribe_events();

    dio.invalidate_all();

    let evt = tokio::time::timeout(Duration::from_millis(200), events.recv())
        .await
        .expect("timed out")
        .expect("bus closed");
    assert!(matches!(evt, DioEvent::Invalidated), "got {evt:?}");
    Ok(())
}

#[tokio::test]
async fn patched_writes_cache_and_publishes() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(master()).await?;
    let mut events = dio.subscribe_events();

    dio.patched("p1", record("patched-name")).await?;

    // Cache reflects the patch.
    let cached = dio
        .cache()
        .get_value("p1")
        .await?
        .expect("cache has the row");
    assert_eq!(
        cached.get("name"),
        Some(&CborValue::Text("patched-name".to_string()))
    );

    // Bus saw the event.
    let evt = tokio::time::timeout(Duration::from_millis(200), events.recv())
        .await
        .expect("timed out")
        .expect("bus closed");
    matches!(evt, DioEvent::RecordChanged { ref id } if id == "p1")
        .then_some(())
        .ok_or_else(|| vantage_core::error!("wrong event", got = format!("{:?}", evt)))?;
    Ok(())
}

#[tokio::test]
async fn handle_event_dispatches_to_on_event_callback() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let calls = Arc::new(AtomicU64::new(0));
    let calls_for_cb = calls.clone();

    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_event(move |dio, evt| {
                let dio = dio.clone();
                let calls = calls_for_cb.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    // Canonical "external system told us about a row" — mirror.
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
    let mut events = dio.subscribe_events();

    // Simulate an external forwarder pumping events into the Dio.
    dio.handle_event(ChangeEvent::Updated {
        id: "ext1".to_string(),
        new: Some(record("from-stream")),
    })
    .await?;

    assert_eq!(calls.load(Ordering::SeqCst), 1, "on_event ran once");

    // Cache has the patched row (via `dio.patched` inside the callback).
    let cached = dio
        .cache()
        .get_value("ext1")
        .await?
        .expect("callback patched cache");
    assert_eq!(
        cached.get("name"),
        Some(&CborValue::Text("from-stream".to_string()))
    );

    // Bus emitted from inside `patched`.
    let evt = tokio::time::timeout(Duration::from_millis(200), events.recv())
        .await
        .expect("timed out")
        .expect("bus closed");
    matches!(evt, DioEvent::RecordChanged { ref id } if id == "ext1")
        .then_some(())
        .ok_or_else(|| vantage_core::error!("wrong event", got = format!("{:?}", evt)))?;
    Ok(())
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
