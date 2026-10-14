//! ValueScenery: one aggregation over a Dio, kept current as the Dio changes.

#[path = "../support/mod.rs"]
mod support;

mod reads;

use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::aggregate::{
    Aggregation, Conditions, Count, Max, Min, Reads, Rows, Sum, Where,
};
use vantage_diorama::{Cache, ChangeEvent, Dio, Lens, Loader};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

use support::{copy_master, memory_lens};

fn cbor_text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn record(name: &str, price: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r.insert("price".to_string(), CborValue::Integer(price.into()));
    r
}

fn items_metadata() -> VistaMetadata {
    VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("price", "i64"))
        .with_id_column("id")
}

fn seeded_shell() -> MockShell {
    MockShell::new()
        .with_metadata(items_metadata())
        .with_record("a", record("alpha", 10))
        .with_record("b", record("beta", 20))
        .with_record("c", record("gamma", 30))
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
    lens.dio(Vista::new("items", seeded_shell())).build().await
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

    let scenery = dio.value_scenery(Count::rows()).open().await?;
    let v = scenery.value().expect("computed");
    assert_eq!(cbor_int(&v), Some(3));
    Ok(())
}

#[tokio::test]
async fn count_where_filters() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio
        .value_scenery(Where::new(
            Conditions::new().eq("name", "alpha"),
            Count::rows(),
        ))
        .open()
        .await?;
    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(1));
    Ok(())
}

#[tokio::test]
async fn sum_over_integer_column() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio.value_scenery(Sum::of("price")).open().await?;
    assert_eq!(scenery.value(), Some(CborValue::Integer(60.into())));
    Ok(())
}

#[tokio::test]
async fn sum_over_a_fractional_column_is_a_float() -> Result<()> {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("n", "f64"))
        .with_id_column("id");
    let n = |v: f64| -> Record<CborValue> {
        [("n".to_string(), CborValue::Float(v))].into_iter().collect()
    };
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", n(1.5))
        .with_record("b", n(2.0));
    let dio = memory_lens(copy_master())
        .dio(Vista::new("floats", shell))
        .build()
        .await?;

    let scenery = dio.value_scenery(Sum::of("n")).open().await?;
    assert_eq!(scenery.value(), Some(CborValue::Float(3.5)));
    Ok(())
}

#[tokio::test]
async fn max_and_min_over_integer_column() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let max = dio.value_scenery(Max::of("price")).open().await?;
    let min = dio.value_scenery(Min::of("price")).open().await?;
    assert_eq!(cbor_int(&max.value().unwrap()), Some(30));
    assert_eq!(cbor_int(&min.value().unwrap()), Some(10));
    Ok(())
}

#[tokio::test]
async fn external_invalidate_triggers_recompute() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio.value_scenery(Count::rows()).open().await?;
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

    let scenery = dio.value_scenery(Count::rows()).open().await?;
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

/// Largest minus smallest price: an aggregation the crate doesn't ship.
struct Spread;

impl Aggregation for Spread {
    type Output = CborValue;

    fn compute(&self, rows: &Rows, _schema: &VistaMetadata) -> CborValue {
        let prices: Vec<i128> = rows
            .values()
            .filter_map(|r| match r.get("price") {
                Some(CborValue::Integer(i)) => Some(i128::from(*i)),
                _ => None,
            })
            .collect();
        match (prices.iter().min(), prices.iter().max()) {
            (Some(lo), Some(hi)) => CborValue::Integer(((hi - lo) as i64).into()),
            _ => CborValue::Null,
        }
    }

    fn reads(&self) -> Reads {
        Reads::Columns(vec!["price".to_string()])
    }
}

#[tokio::test]
async fn custom_aggregate_runs() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio.value_scenery(Spread).open().await?;

    // 30 - 10
    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(20));
    Ok(())
}

#[tokio::test]
async fn scenery_outlives_dio_handle_drop() -> Result<()> {
    let tmp = TempDir::new().unwrap();
    let dio = seeded_dio(&tmp).await?;

    let scenery = dio.value_scenery(Count::rows()).open().await?;
    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(3));

    drop(dio);
    assert_eq!(cbor_int(&scenery.value().unwrap()), Some(3));
    Ok(())
}


