//! Stage 7 demo: ValueScenery as a "menu-bar badge" surface.
//!
//! A polling loop watches three scenery types (Count, Sum,
//! Custom-as-average) and prints them on every generation bump. We
//! mutate the cache from outside and watch the values follow.
//!
//! Run with:
//!   cargo run -p vantage-diorama --example badge_demo

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Lens, ValueScenery};
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

fn master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String"))
        .with_column(Column::new("price", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", record("alpha", 10))
        .with_record("b", record("beta", 20));
    Vista::new("items", Box::new(shell))
}

fn show(
    label: &str,
    count: &Arc<dyn ValueScenery>,
    sum: &Arc<dyn ValueScenery>,
    avg: &Arc<dyn ValueScenery>,
) {
    let fmt = |v: Option<CborValue>| match v {
        Some(CborValue::Integer(i)) => format!("{}", i128::from(i)),
        Some(other) => format!("{other:?}"),
        None => "—".to_string(),
    };
    println!(
        "[{label}] count={} sum={} avg={}",
        fmt(count.value()),
        fmt(sum.value()),
        fmt(avg.value())
    );
}

async fn wait_for_bump(
    rx: &mut tokio::sync::watch::Receiver<vantage_diorama::Generation>,
    last: u64,
) -> u64 {
    tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            if u64::from(*rx.borrow_and_update()) > last {
                return u64::from(*rx.borrow());
            }
            rx.changed().await.ok();
        }
    })
    .await
    .unwrap_or(last)
}

#[tokio::main]
async fn main() -> Result<()> {
    let tmp = TempDir::new().expect("tempdir");

    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await
                }
            })
            .build()
            .expect("build lens"),
    );

    let dio = lens.make_dio(master()).await?;
    let count = dio.value_scenery().count().open().await?;
    let sum = dio.value_scenery().sum("price").open().await?;
    let avg = dio
        .value_scenery()
        .custom(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.cache().list_values().await?;
                let mut s: i64 = 0;
                let mut n: i64 = 0;
                for (_, r) in rows {
                    if let Some(CborValue::Integer(i)) = r.get("price") {
                        s += i64::try_from(*i).unwrap_or(0);
                        n += 1;
                    }
                }
                let v = if n == 0 { 0 } else { s / n };
                Ok(CborValue::Integer(v.into()))
            }
        })
        .open()
        .await?;

    show("initial", &count, &sum, &avg);

    // Add a third row outside; all three scenery values shift.
    let mut count_rx = count.subscribe();
    let mut sum_rx = sum.subscribe();
    let mut avg_rx = avg.subscribe();
    let last_count = u64::from(*count_rx.borrow_and_update());
    let last_sum = u64::from(*sum_rx.borrow_and_update());
    let last_avg = u64::from(*avg_rx.borrow_and_update());
    dio.cache().insert_value("c", &record("gamma", 30)).await?;
    dio.notify_record_changed("c");
    let _ = wait_for_bump(&mut count_rx, last_count).await;
    let _ = wait_for_bump(&mut sum_rx, last_sum).await;
    let _ = wait_for_bump(&mut avg_rx, last_avg).await;
    show("after insert(gamma=30)", &count, &sum, &avg);

    Ok(())
}
