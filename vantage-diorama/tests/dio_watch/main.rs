//! A Dio built over another Dio's narrowed facade follows the parent live:
//! `.live()` subscribes to the parent's event bus through the facade, and a
//! parent change that moves a row across the filter shows up downstream.
//!
//! The follow recovers: a child that lags behind its parent re-reads the set,
//! and a change made while the child's eager start copies the set is applied
//! over the copy.

mod copying;
mod following;
mod recovery;

use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::sync::broadcast;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Cache, ChangeEvent, Dio, DioEvent, Lens, Loader};
use vantage_types::Record;
use vantage_vista::{Column, FilterOp, Vista, VistaMetadata, mocks::MockShell};

fn shipment(status: &str) -> Record<CborValue> {
    let mut record = Record::new();
    record.insert("status".to_string(), CborValue::Text(status.to_string()));
    record
}

fn noted(status: &str, note: &str) -> Record<CborValue> {
    let mut record = shipment(status);
    record.insert("note".to_string(), CborValue::Text(note.to_string()));
    record
}

fn master() -> MockShell {
    MockShell::new().with_metadata(
        VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("status", "String"))
            .with_column(Column::new("note", "String"))
            .with_id_column("id"),
    )
}

/// An eager loader that hands the whole master to `reconcile`.
fn copy_master() -> Loader {
    Loader::eager(|cx| async move {
        let rows = cx.master().list_values().await?;
        cx.reconcile(rows).await
    })
}

/// An eager Dio over `master`, following it live when `live` is set.
async fn eager_dio(master: Vista, live: bool) -> Result<Dio> {
    let lens = Lens::builder()
        .cache(Cache::memory())
        .loader(copy_master())
        .build()?;
    let builder = lens.dio(master);
    if live {
        builder.live().build().await
    } else {
        builder.build().await
    }
}

/// The parent's facade narrowed to the shipments in transit.
fn in_transit(parent: &Dio) -> Result<Vista> {
    parent
        .cached()
        .with_condition("status", FilterOp::Eq, "In transit")
}

/// A parent Dio and the shell behind its master.
struct Parent {
    dio: Dio,
    shell: MockShell,
}

impl Parent {
    async fn over(shell: MockShell) -> Result<Self> {
        let dio = eager_dio(Vista::new("shipment", shell.clone()), false).await?;
        Ok(Self { dio, shell })
    }

    /// An upstream change: the master stores `record`, and the change is
    /// pushed into the parent.
    async fn upsert(&self, id: &str, record: Record<CborValue>) -> Result<()> {
        self.shell.set_record(id, record.clone());
        let new = Some(record);
        self.dio
            .apply_change(ChangeEvent::Updated { id: id.into(), new })
            .await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.shell.remove_record(id);
        self.dio
            .apply_change(ChangeEvent::Deleted { id: id.into() })
            .await
    }
}

/// Wait until `pred` holds for `dio`, re-checking on every event its bus
/// publishes. The timeout bounds a hang; it is not a speed assertion.
async fn wait_until<F>(dio: &Dio, mut bus: broadcast::Receiver<DioEvent>, pred: F)
where
    F: AsyncFn(&Dio) -> bool,
{
    let wait = async {
        while !pred(dio).await {
            let _ = bus.recv().await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("the follow never caught up");
}

async fn note_of(dio: &Dio, id: &str) -> Option<CborValue> {
    let row = dio.cached().get_value(id).await.ok().flatten();
    row.and_then(|r| r.get("note").cloned())
}

async fn holds(dio: &Dio, id: &str) -> bool {
    dio.cached().get_value(id).await.ok().flatten().is_some()
}
