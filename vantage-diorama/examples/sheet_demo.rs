//! Stage 6 demo: RecordScenery as a "detail sheet" surface.
//!
//! A text-mode polling loop prints the current record + status on each
//! generation bump. We then apply external changes and watch the loop
//! pick them up.
//!
//! Run with:
//!   cargo run -p vantage-diorama --example sheet_demo

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tempfile::TempDir;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, ChangeEvent, Lens, Loader, LoaderCx, RecordScenery};
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
        .with_record("c1", record("coffee", 5));
    Vista::new("items", shell)
}

fn print_scenery(label: &str, scenery: &Arc<dyn RecordScenery>) {
    let status = scenery.status();
    print!("[{label}] status={status:?} ");
    if let Some(r) = scenery.record() {
        let name = r.record.get("name").and_then(|v| match v {
            CborValue::Text(s) => Some(s.as_str()),
            _ => None,
        });
        let price = r.record.get("price").and_then(|v| match v {
            CborValue::Integer(i) => Some(i128::from(*i)),
            _ => None,
        });
        println!("record={}/${}", name.unwrap_or(""), price.unwrap_or(0));
    } else {
        println!("record=<none>");
    }
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

    let lens = Lens::builder()
        .cache(Cache::redb(tmp.path().join("cache.redb")))
        .loader(Loader::eager(|cx: LoaderCx| async move {
            let rows = cx.master().list_values().await?;
            cx.cache().insert_values(rows).await
        }))
        .build()?;

    let dio = lens.dio(master()).build().await?;

    // Open the sheet for a known row.
    let sheet = dio.record_scenery("c1").await?;
    print_scenery("initial", &sheet);

    // External system renames the row.
    let mut gen_rx = sheet.subscribe();
    let last = u64::from(*gen_rx.borrow_and_update());
    dio.apply_change(updated("c1", record("espresso", 5)))
        .await?;
    wait_for_bump(&mut gen_rx, last).await;
    print_scenery("after the update", &sheet);

    // Open another sheet for a missing id — NotFound, no master fetch.
    let missing = dio.record_scenery("nope").await?;
    print_scenery("for missing id", &missing);

    // Land that row through the change ingress; the open sheet observes it.
    dio.apply_change(updated("nope", record("appeared", 2)))
        .await?;
    let _ = wait_for_bump(&mut missing.subscribe(), 0).await;
    print_scenery("after the update (missing → appeared)", &missing);

    Ok(())
}

fn updated(id: &str, record: Record<CborValue>) -> ChangeEvent {
    ChangeEvent::Updated {
        id: id.to_string(),
        new: Some(record),
    }
}
