//! Stage 5 demo: TableScenery as a "render loop" surface.
//!
//! A text-mode polling loop watches the Scenery's generation channel
//! and reprints every row on each bump. We then prove reactivity by
//! mutating the cache from outside and watching the loop pick it up.
//!
//! Run with:
//!   cargo run -p vantage-diorama --example scenery_basic

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Lens, SortDir, TableScenery};
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
        .with_record("c1", record("coffee", 5))
        .with_record("c2", record("tea", 3))
        .with_record("c3", record("juice", 4));
    Vista::new("items", Box::new(shell))
}

fn print_scenery(label: &str, scenery: &Arc<dyn TableScenery>) {
    println!("\n[{label}] {} rows", scenery.row_count());
    for i in 0..scenery.row_count() {
        let r = scenery.row(i).unwrap();
        let name = r.record.get("name").and_then(|v| match v {
            CborValue::Text(s) => Some(s.as_str()),
            _ => None,
        });
        let price = r.record.get("price").and_then(|v| match v {
            CborValue::Integer(i) => Some(i128::from(*i)),
            _ => None,
        });
        println!("  {}: {} (${})", i, name.unwrap_or(""), price.unwrap_or(0));
    }
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
    let scenery: Arc<dyn TableScenery> =
        dio.table_scenery().sort("price", SortDir::Asc).open().await?;

    print_scenery("initial (sorted by price asc)", &scenery);

    // Switch to descending sort.
    let mut gen_rx = scenery.subscribe();
    let last = u64::from(*gen_rx.borrow_and_update());
    scenery.set_sort(Some("price".to_string()), SortDir::Desc);
    let _ = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            if u64::from(*gen_rx.borrow_and_update()) > last {
                break;
            }
            gen_rx.changed().await.ok();
        }
    })
    .await;
    print_scenery("after set_sort(price, desc)", &scenery);

    // External system tells us about a new row.
    dio.cache().insert_value("c4", &record("water", 1)).await?;
    dio.invalidate_record("c4");
    let last = u64::from(*gen_rx.borrow_and_update());
    let _ = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            if u64::from(*gen_rx.borrow_and_update()) > last {
                break;
            }
            gen_rx.changed().await.ok();
        }
    })
    .await;
    print_scenery("after external insert + invalidate", &scenery);

    Ok(())
}
