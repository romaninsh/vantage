//! Stage 5: TableScenery — reactive ordered-rows view.

mod changes;
mod opening;
mod shaping;
#[path = "../support/mod.rs"]
mod support;

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_diorama::{Lens, TableScenery};
use vantage_types::Record;
use vantage_vista::{Column, ColumnFlag, Vista, VistaMetadata, mocks::MockShell};

use support::chunk::redb_lens;
use support::copy_master;

fn cbor_text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

fn record(name: &str, price: i64) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert("name".to_string(), cbor_text(name));
    r.insert("price".to_string(), CborValue::Integer(price.into()));
    r
}

/// A seeded row: `record` plus its `id`, a `status` and an integer `n`.
fn seeded(id: &str, name: &str, price: i64, status: &str, n: i64) -> Record<CborValue> {
    let mut r = record(name, price);
    r.insert("id".to_string(), cbor_text(id));
    r.insert("status".to_string(), cbor_text(status));
    r.insert("n".to_string(), CborValue::Integer(n.into()));
    r
}

/// Build a MockShell-backed master pre-seeded with three rows.
fn seeded_master() -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag(ColumnFlag::Orderable))
        .with_column(Column::new("price", "i64"))
        .with_column(Column::new("status", "String"))
        .with_column(Column::new("n", "i64"))
        .with_id_column("id");
    let shell = MockShell::new()
        .with_metadata(metadata)
        .with_record("a", seeded("a", "alpha", 30, "open", 1))
        .with_record("b", seeded("b", "beta", 10, "closed", 5))
        .with_record("c", seeded("c", "gamma", 20, "open", 9));
    Vista::new("items", shell)
}

/// Ids of the rows `scenery` shows, in row order.
fn shown_ids(scenery: &Arc<dyn TableScenery>) -> Vec<String> {
    (0..scenery.row_count())
        .filter_map(|i| scenery.row(i))
        .filter_map(|row| match row.record.get("id") {
            Some(CborValue::Text(id)) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// Build a Lens whose eager start copies the master into the cache so
/// the Scenery has something to read.
async fn build_lens(cache_path: std::path::PathBuf) -> Result<Arc<Lens>> {
    Ok(redb_lens(cache_path, copy_master()))
}

/// Wait for the Scenery's generation to advance past `current`.
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
