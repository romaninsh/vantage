//! The write queue's edge cases: a caller cancelled mid-stage, a failure
//! under a flash stacked on the same row, a writer that writes its own Dio,
//! a writer that panics, a writer over a natively narrowed master, and the
//! settle event a two-pass view needs to follow membership.

mod membership;
mod staging;
#[path = "../support/mod.rs"]
mod support;
mod writers;

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::sync::broadcast;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{ChangeFlash, Dio, DioEvent, Lens};
use vantage_types::Record;
use vantage_vista::{Vista, mocks::MockShell};

use support::{meta, record};

fn items() -> MockShell {
    MockShell::new()
        .with_metadata(meta(&["id", "name"]))
        .with_record("a", record(&[("id", "a"), ("name", "one")]))
}

fn name_of(row: Option<Record<CborValue>>) -> Option<String> {
    match row?.get("name") {
        Some(CborValue::Text(name)) => Some(name.clone()),
        _ => None,
    }
}

/// A Dio over [`items`] on `lens`, its cache holding the master's rows.
async fn seeded(lens: &Arc<Lens>, master: Vista) -> Result<Dio> {
    let dio = lens.dio(master).build().await?;
    dio.reconcile(dio.master().list_values().await?).await?;
    Ok(dio)
}

fn rename(name: &str) -> ChangeFlash {
    ChangeFlash::partial("a", record(&[("name", name)]))
}

/// Wait until the bus announces a staged flash.
async fn staged(bus: &mut broadcast::Receiver<DioEvent>) {
    let wait = async { while !matches!(bus.recv().await, Ok(DioEvent::WritePending { .. })) {} };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("the flash was never staged");
}
