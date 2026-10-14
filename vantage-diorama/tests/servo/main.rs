//! `Servo` — the editing companion as a servo loop over a record.
//!
//! `data` is the commanded setpoint, `baseline` the measured upstream state,
//! and the dirty set is the error signal (`data ≠ baseline`, computed by
//! diff). Untouched fields run in continuous tracking — upstream changes
//! update them live and they stay clean; touched fields lock and hold.
//! `flash()` freezes the error signal into an immutable [`ChangeFlash`]
//! carrying only the changed fields.

mod drafts;
mod failures;
mod flashes;
mod setpoints;
#[path = "../support/mod.rs"]
mod support;

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ciborium::Value as CborValue;
use futures::future::BoxFuture;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Cache, ChangeEvent, ChangeFlash, Dio, FlashOutcome, Lens, LoaderCx};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

use support::text;

fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}

fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    let mut r = Record::new();
    for (k, v) in pairs {
        r.insert((*k).to_string(), v.clone());
    }
    r
}

/// Product master: one seeded row `p1` and a live handle to mutate the
/// store out-of-band (the "another process wrote to the backend" lever).
fn product_shell() -> MockShell {
    MockShell::new().with_record(
        "p1",
        rec(&[
            ("id", text("p1")),
            ("name", text("Coffee")),
            ("price", int(3)),
            ("stock", int(10)),
        ]),
    )
}

fn product_vista(shell: &MockShell) -> Vista {
    let metadata = VistaMetadata::new()
        .with_column(Column::new("id", "String").with_flag("id"))
        .with_column(Column::new("name", "String").with_flag("title"))
        .with_column(Column::new("price", "int"))
        .with_column(Column::new("stock", "int"))
        .with_id_column("id");
    Vista::new("products", shell.clone().with_metadata(metadata))
}

/// Dio over an in-memory cache seeded from the master's current rows.
async fn dio_over(shell: &MockShell) -> Result<Dio> {
    let lens = Lens::builder().cache(Cache::memory()).build()?;
    seeded(lens.dio(product_vista(shell)).build().await?).await
}

/// [`dio_over`] whose Lens routes every write through `writer`.
async fn routed_dio<F, Fut>(shell: &MockShell, writer: F) -> Result<Dio>
where
    F: Fn(LoaderCx, ChangeFlash) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<FlashOutcome>> + Send + 'static,
{
    let lens = Lens::builder()
        .cache(Cache::memory())
        .writer(writer)
        .build()?;
    seeded(lens.dio(product_vista(shell)).build().await?).await
}

async fn seeded(dio: Dio) -> Result<Dio> {
    dio.reconcile(dio.master().list_values().await?).await?;
    Ok(dio)
}

/// A writer that saves each flash into the Dio's master, failing while
/// `reject` is set.
fn saving_writer(
    reject: Arc<AtomicBool>,
) -> impl Fn(LoaderCx, ChangeFlash) -> BoxFuture<'static, Result<FlashOutcome>> + Send + Sync {
    move |cx, flash| {
        let reject = reject.clone();
        let master = cx.master().clone();
        Box::pin(async move {
            if reject.load(Ordering::SeqCst) {
                return Err(vantage_core::error!("route rejected the flash"));
            }
            flash.active_record(&master)?.save().await?;
            Ok(FlashOutcome::applied())
        })
    }
}

/// An upstream change to `p1`, pushed into the Dio.
async fn upstream(dio: &Dio, name: &str, stock: i64) -> Result<()> {
    let row = rec(&[
        ("id", text("p1")),
        ("name", text(name)),
        ("price", int(3)),
        ("stock", int(stock)),
    ]);
    dio.apply_change(ChangeEvent::Updated {
        id: "p1".into(),
        new: Some(row),
    })
    .await
}
