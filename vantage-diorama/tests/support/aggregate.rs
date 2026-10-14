//! A live source Dio backed by a mutable in-memory master.

use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::aggregate::Reduce;
#[cfg(feature = "diagnostics")]
use vantage_diorama::Refresher;
use vantage_diorama::{Cache, Dio, Generation, Lens, LensBuilder, Loader, Tuning};
use vantage_types::Record;
use vantage_vista::{Column, Vista, VistaMetadata, mocks::MockShell};

/// Aggregate debounce used across the suite. Short enough to keep tests
/// quick, long enough that a burst genuinely coalesces.
pub const DEBOUNCE: Duration = Duration::from_millis(40);

/// Long enough for a leading-edge recompute, the cooldown, and the trailing
/// flush to all land.
pub async fn settle() {
    tokio::time::sleep(DEBOUNCE * 5).await;
}

/// Source tuning on the suite's [`DEBOUNCE`]: an aggregation recomputes on
/// its source Dio's Lens.
pub fn tuning() -> Tuning {
    Tuning::default().with_aggregate_debounce(DEBOUNCE)
}

/// An aggregate's integer value, or `None` for anything else.
pub fn as_i64(value: Option<CborValue>) -> Option<i64> {
    match value {
        Some(CborValue::Integer(i)) => Some(i128::from(i) as i64),
        _ => None,
    }
}

pub fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}

pub fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

pub fn text(s: &str) -> CborValue {
    CborValue::Text(s.to_string())
}

/// Orders and revenue per group: the "custom Rust callback" reducer a
/// consumer writes.
pub fn revenue()
-> Reduce<impl Fn(&CborValue, &[&Record<CborValue>], &VistaMetadata) -> Record<CborValue>> {
    Reduce::new(
        vec![Column::new("orders", "i64"), Column::new("revenue", "i64")],
        |_key, rows, _schema| {
            let total: i64 = rows
                .iter()
                .filter_map(|r| r.get("amount"))
                .filter_map(|v| match v {
                    CborValue::Integer(i) => Some(i128::from(*i) as i64),
                    _ => None,
                })
                .sum();
            let mut out = Record::new();
            out.insert("orders".to_string(), int(rows.len() as i64));
            out.insert("revenue".to_string(), int(total));
            out
        },
    )
}

/// A master whose rows a test can mutate mid-run.
pub fn master(columns: &[(&str, &str)]) -> MockShell {
    let mut metadata =
        VistaMetadata::new().with_column(Column::new("id", "String").with_flag("id"));
    for (name, ty) in columns {
        metadata = metadata.with_column(Column::new(*name, *ty));
    }
    MockShell::new().with_metadata(metadata.with_id_column("id"))
}

/// An eager source Dio: the eager loader copies the master into the cache,
/// and a refresh re-runs it, reconciling the cache with the master. Mutating
/// the shell then calling [`Dio::refresh`] is how a test simulates the
/// datasource changing.
pub async fn source(shell: MockShell) -> Dio {
    let lens = eager_lens().build().expect("source lens builds");
    open_items(&lens, shell).await
}

/// Same as [`source`], but with the official debug stream turned on for
/// this datasource — the tap an aggregate spawned over this Dio inherits.
#[cfg(feature = "diagnostics")]
pub async fn source_with_debug(shell: MockShell, ds_name: &str) -> Dio {
    let lens = eager_lens()
        .debug_datasource(ds_name)
        .build()
        .expect("source lens builds");
    open_items(&lens, shell).await
}

/// A debug-tapped source whose refresher always fails.
///
/// A value scenery's `refresh_source` always calls the source's `refresh()`
/// *and then* nudges the engine loop directly — the nudge is there for a
/// source with no refresh route, so the value doesn't just sit inert. A
/// working refresh makes those two signals arrive together (the
/// refresh's own `DatasetChanged` and the nudge, both ready before the
/// engine loop gets a chance to poll), which makes the wake a genuine race:
/// `trigger` could land as either. Failing the refresh suppresses
/// `DatasetChanged` (`Dio::refresh` only emits it on `Ok`) while the nudge
/// still fires unconditionally, so a test wanting the `"nudge"` trigger on
/// its own, deterministically, needs a source like this one.
#[cfg(feature = "diagnostics")]
pub async fn source_with_debug_and_failing_refresh(shell: MockShell, ds_name: &str) -> Dio {
    let lens = eager_lens()
        .debug_datasource(ds_name)
        .refresher(Refresher::new(|_cx| async move {
            Err(vantage_core::error!(
                "test: refresh always fails to isolate the nudge trigger"
            ))
        }))
        .build()
        .expect("source lens builds");
    open_items(&lens, shell).await
}

/// An in-memory Lens on [`tuning`] whose eager loader reconciles the cache
/// with the whole master.
fn eager_lens() -> LensBuilder {
    Lens::builder()
        .cache(Cache::memory())
        .tuning(tuning())
        .loader(Loader::eager(|cx| async move {
            let rows = cx.master().list_values().await?;
            cx.reconcile(rows).await
        }))
}

async fn open_items(lens: &Arc<Lens>, shell: MockShell) -> Dio {
    let vista = Vista::new("items", shell);
    lens.dio(vista).build().await.expect("source dio")
}

/// Counts generation bumps on a scenery's watch channel.
pub struct BumpCounter {
    rx: tokio::sync::watch::Receiver<Generation>,
    seen: u64,
    count: usize,
}

impl BumpCounter {
    pub fn new(rx: tokio::sync::watch::Receiver<Generation>) -> Self {
        let seen = u64::from(*rx.borrow());
        Self { rx, seen, count: 0 }
    }

    /// Bumps observed since the last call.
    pub fn drain(&mut self) -> usize {
        let current = u64::from(*self.rx.borrow_and_update());
        if current > self.seen {
            self.count += (current - self.seen) as usize;
            self.seen = current;
        }
        std::mem::take(&mut self.count)
    }
}
