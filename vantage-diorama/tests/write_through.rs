//! Stage 3 end-to-end: write queue, on_write callback, default-to-master,
//! refresh task, manual refresh, error propagation.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::{ReadableValueSet, WritableValueSet};
use vantage_diorama::{DioEvent, Lens, WriteOp};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_id_column("id");
    Vista::new(
        "items",
        Box::new(MockShell::new().with_metadata(metadata)),
    )
}

fn record(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), CborValue::Text(name.to_string()));
    r
}

#[tokio::test]
async fn on_write_writes_both_master_and_cache() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_write(|dio, op| {
                let dio = dio.clone();
                async move {
                    // User's callback decides the routing — here: write to
                    // master, then mirror into cache so reads see it.
                    match op {
                        WriteOp::Insert { id, record } => {
                            dio.master().insert_value(&id, &record).await?;
                            dio.cache().insert_value(&id, &record).await?;
                        }
                        WriteOp::Delete { id } => {
                            dio.master().delete(&id).await?;
                            dio.cache().delete_value(&id).await?;
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
    let facade = dio.vista();

    facade.insert_value(&"a".to_string(), &record("apple")).await?;
    facade.insert_value(&"b".to_string(), &record("banana")).await?;

    // Wait a beat for the worker to drain.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Master saw both writes.
    let m = dio.master().list_values().await?;
    assert_eq!(m.len(), 2);

    // Cache reads served the same data.
    let rows = facade.list_values().await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows["a"].get("name"),
        Some(&CborValue::Text("apple".to_string()))
    );

    // Capability flips reflect on_write registration.
    let caps = facade.capabilities();
    assert!(caps.can_insert && caps.can_update && caps.can_delete);
    Ok(())
}

#[tokio::test]
async fn default_write_goes_straight_to_master() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            // No on_write — worker default-writes to master.
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(master()).await?;
    let facade = dio.vista();

    facade.insert_value(&"x".to_string(), &record("xerox")).await?;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Master got it via the default path.
    let m = dio.master().list_values().await?;
    assert_eq!(m.len(), 1);
    assert_eq!(
        m["x"].get("name"),
        Some(&CborValue::Text("xerox".to_string()))
    );

    // Cache wasn't auto-mirrored — facade reads see nothing.
    assert_eq!(facade.list_values().await?.len(), 0);
    Ok(())
}

#[tokio::test]
async fn on_write_error_publishes_write_failed_event() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_write(|_dio, _op| async move {
                Err(vantage_core::error!("user callback says no"))
            })
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(master()).await?;
    let mut events = dio.subscribe_events();
    let facade = dio.vista();

    facade
        .insert_value(&"bad".to_string(), &record("doomed"))
        .await?;

    // Worker fires the callback, callback errors → WriteFailed published.
    let evt = tokio::time::timeout(Duration::from_millis(500), events.recv())
        .await
        .expect("timed out waiting for WriteFailed")
        .expect("event bus closed");
    match evt {
        DioEvent::WriteFailed { id, error } => {
            assert_eq!(id.as_deref(), Some("bad"));
            assert!(error.contains("user callback says no"));
        }
        other => panic!("expected WriteFailed, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn refresh_task_fires_periodically() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let counter = Arc::new(AtomicU64::new(0));
    let counter_for_cb = counter.clone();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .refresh_every(Duration::from_millis(40))
            .on_refresh(move |_dio| {
                let c = counter_for_cb.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
            .build()
            .expect("build lens"),
    );
    let _dio = lens.make_dio(master()).await?;

    // Two intervals (skipping the immediate tick) → at least one fire by 120ms.
    tokio::time::sleep(Duration::from_millis(180)).await;
    assert!(
        counter.load(Ordering::SeqCst) >= 2,
        "expected refresh ≥ 2 fires, got {}",
        counter.load(Ordering::SeqCst)
    );
    Ok(())
}

#[tokio::test]
async fn manual_refresh_fires_and_propagates_errors() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let counter = Arc::new(AtomicU64::new(0));
    let counter_for_cb = counter.clone();
    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_refresh(move |_dio| {
                let c = counter_for_cb.clone();
                async move {
                    let n = c.fetch_add(1, Ordering::SeqCst);
                    if n == 1 {
                        Err(vantage_core::error!("planned failure"))
                    } else {
                        Ok(())
                    }
                }
            })
            .build()
            .expect("build lens"),
    );
    let dio = lens.make_dio(master()).await?;

    // First fire: ok.
    dio.refresh().await?;
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    // Second fire: callback returns Err, surfaces to the caller.
    let err = dio.refresh().await.unwrap_err();
    assert!(err.to_string().contains("planned failure"));
    assert_eq!(counter.load(Ordering::SeqCst), 2);
    Ok(())
}
