//! Stage 7: ValueScenery — single-scalar reactive view.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, ChangeEvent, Dio, Lens, Loader};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

fn cbor_text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn record(name: &str, price: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r.insert("price".to_string(), CborValue::Integer(price.into()));
    r
}

fn seeded_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("price", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", record("alpha", 10))
        .with_record("b", record("beta", 20))
        .with_record("c", record("gamma", 30));
    Vista::new("items", shell)
}

/// A Dio over the seeded master on a redb cache in `tmp`, copied whole by an
/// eager start.
async fn seeded_dio(tmp: &TempDir) -> Result<Dio> {
    let lens = Lens::builder()
        .cache(Cache::redb(tmp.path().join("cache.redb")))
        .loader(Loader::eager(|cx| async move {
            let rows = cx.master().list_values().await?;
            cx.reconcile(rows).await
        }))
        .build()?;
    lens.dio(seeded_master()).build().await
}

async fn wait_for_gen(
    rx: &mut tokio::sync::watch::Receiver<vantage_diorama::Generation>,
    current: u64,
) -> u64 {
    // Bounds a hang, not a speed — see `support/chunk.rs::wait_for_gen`.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if u64::from(*rx.borrow_and_update()) > current {
                return u64::from(*rx.borrow());
            }
            rx.changed().await.expect("watch channel closed");
        }
    })
    .await
    .expect("timed out waiting for generation bump")
}

fn cbor_int(v: &CborValue) -> Option<i128> {
    if let CborValue::Integer(i) = v {
        Some(i128::from(*i))
    } else {
        None
    }
}

#[tokio::test]
async fn count_returns_cache_size() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio.value_scenery().count().open().await?;
    let v = scenery.value().expect("computed");
    assert_eq!(cbor_int(&v), Some(3));
    Ok(())
}

#[tokio::test]
async fn count_where_filters() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio
        .value_scenery()
        .count_where(vec![("name".to_string(), cbor_text("alpha"))])
        .open()
        .await?;
    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(1));
    Ok(())
}

#[tokio::test]
async fn sum_over_integer_column() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio.value_scenery().sum("price").open().await?;
    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(60));
    Ok(())
}

#[tokio::test]
async fn max_and_min_over_integer_column() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let max = dio.value_scenery().max("price").open().await?;
    let min = dio.value_scenery().min("price").open().await?;
    assert_eq!(cbor_int(&max.value().unwrap()), Some(30));
    assert_eq!(cbor_int(&min.value().unwrap()), Some(10));
    Ok(())
}

#[tokio::test]
async fn external_invalidate_triggers_recompute() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio.value_scenery().count().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    let delta = Some(record("delta", 5));
    dio.apply_change(ChangeEvent::Inserted {
        id: "d".into(),
        new: delta,
    })
    .await?;
    wait_for_gen(&mut gen_rx, initial).await;

    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(4));
    Ok(())
}

#[tokio::test]
async fn unchanged_value_does_not_bump_generation() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio.value_scenery().count().open().await?;
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    // Touch the bus without changing the count.
    dio.apply_change(ChangeEvent::Updated {
        id: "nonexistent".into(),
        new: None,
    })
    .await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let after = u64::from(*gen_rx.borrow_and_update());
    assert_eq!(after, initial, "count unchanged → no generation bump");
    Ok(())
}

#[tokio::test]
async fn custom_aggregate_runs() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio
        .value_scenery()
        .custom(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.cached().list_values().await?;
                // Average price as integer.
                let mut sum: i64 = 0;
                let mut n: i64 = 0;
                for (_, rec) in rows {
                    if let Some(CborValue::Integer(i)) = rec.get("price") {
                        sum += i64::try_from(*i).unwrap_or(0);
                        n += 1;
                    }
                }
                let avg = if n == 0 { 0 } else { sum / n };
                Ok(CborValue::Integer(avg.into()))
            }
        })
        .open()
        .await?;

    // (10 + 20 + 30) / 3 = 20
    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(20));
    Ok(())
}

#[tokio::test]
async fn custom_aggregate_error_preserves_last_value() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    // Counter so the custom aggregate alternates between OK and Err.
    let calls = Arc::new(AtomicU64::new(0));
    let calls_for_cb = calls.clone();
    let scenery = dio
        .value_scenery()
        .custom(move |_dio| {
            let calls = calls_for_cb.clone();
            async move {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Ok(CborValue::Integer(42.into()))
                } else {
                    Err(vantage_core::error!("planned failure"))
                }
            }
        })
        .open()
        .await?;

    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(42));
    let mut gen_rx = scenery.subscribe();
    let initial = u64::from(*gen_rx.borrow_and_update());

    // Trigger a recompute that will fail: the set is re-read.
    dio.apply_change(ChangeEvent::Invalidated).await?;
    wait_for_gen(&mut gen_rx, initial).await;

    // Value preserved; status flipped to Error.
    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(42));
    match scenery.status() {
        vantage_diorama::ValueStatus::Error(msg) => assert!(msg.contains("planned failure")),
        other => panic!("expected Error, got {other:?}"),
    }
    Ok(())
}

#[tokio::test]
async fn scenery_outlives_dio_handle_drop() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio.value_scenery().count().open().await?;
    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(3));

    drop(dio);
    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(3));
    Ok(())
}
