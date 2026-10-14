//! RecordScenery — single-row reactive view.

mod builder;
mod reads;
#[path = "../support/mod.rs"]
mod support;
mod writes;

use std::sync::Arc;
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
        .with_record("a", record("alpha", 30))
        .with_record("b", record("beta", 10));
    Vista::new("items", shell)
}

/// An eager loader copying the whole master into the cache.
fn copy_master() -> Loader {
    Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    })
}

fn build_lens(cache: Cache) -> Arc<Lens> {
    Lens::builder()
        .cache(cache)
        .loader(copy_master())
        .build()
        .expect("build lens")
}

async fn redb_dio(tmp: &TempDir) -> Result<Dio> {
    let lens = build_lens(Cache::redb(tmp.path().join("cache.redb")));
    lens.dio(seeded_master()).build().await
}

/// An update to `id`: carrying the row, or notify-only with `None`.
fn updated(id: &str, new: Option<Record<CborValue>>) -> ChangeEvent {
    ChangeEvent::Updated {
        id: id.to_string(),
        new,
    }
}

fn partial_name(name: &str) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r
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
