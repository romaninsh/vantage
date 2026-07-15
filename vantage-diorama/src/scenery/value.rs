//! `ValueScenery` — reactive single-scalar view.
//!
//! Wraps one [`Aggregate`] over the Dio's cache and bumps a watch
//! channel when the computed value (or status) changes. Used by
//! menu-bar badges, dashboard counters, anywhere "one number that
//! refreshes" is the UI shape.
//!
//! Status + value transitions in v1:
//!
//! | Trigger                              | Status     | Value                  | Bump |
//! |--------------------------------------|------------|------------------------|------|
//! | Open                                 | `Loading`  | `None`                 | -    |
//! | First compute OK                     | `Fresh`    | new                    | yes  |
//! | Recompute OK, unchanged              | `Fresh`    | unchanged              | no   |
//! | Recompute OK, changed                | `Fresh`    | new                    | yes  |
//! | Recompute failed                     | `Error(_)` | **last good preserved**| yes  |
//! | Recompute OK after failure           | `Fresh`    | new                    | yes  |
//!
//! Deliberately *not* in v1:
//!
//! - **Push-down via vista 5b** — `Sum`/`Max`/`Min` scan the cache
//!   locally instead of calling `dio.vista().get_sum(field)` etc.
//! - **Float arithmetic** — `Sum`/`Max`/`Min` recognize CBOR
//!   integers only; any other type for the target field yields
//!   `Error("non-integer field")`.
//! - **Memoization across multiple ValueSceneries with the same
//!   aggregate** — users share the `Arc<dyn ValueScenery>` if they
//!   care.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};

use ciborium::Value as CborValue;
use tokio::sync::{broadcast, watch};
use vantage_core::{Result, error};

use crate::dio::{Dio, DioEvent, DioInner, Generation};

#[derive(Debug, Clone)]
pub enum ValueStatus {
    Fresh,
    Stale,
    Loading,
    Error(String),
}

/// Reactive view onto a single scalar — typically an aggregate
/// (`COUNT`, `SUM`) computed against the underlying Dio.
pub trait ValueScenery: Send + Sync {
    fn value(&self) -> Option<CborValue>;
    fn status(&self) -> ValueStatus;

    fn request_refresh(&self);
    fn subscribe(&self) -> watch::Receiver<Generation>;
}

/// Future returned by a custom aggregate closure.
pub type CustomAggregateFuture<'a> = Pin<Box<dyn Future<Output = Result<CborValue>> + Send + 'a>>;

/// Boxed user-supplied aggregate. HRTB so the same closure can be
/// invoked against any `&Dio` borrow lifetime.
pub type CustomAggregate =
    Box<dyn for<'a> Fn(&'a Dio) -> CustomAggregateFuture<'a> + Send + Sync + 'static>;

/// Wrap a user closure into a [`CustomAggregate`]. Canonical user
/// pattern: `move |dio| { let dio = dio.clone(); async move { ... } }`.
pub fn boxed_custom_aggregate<F, Fut>(f: F) -> CustomAggregate
where
    F: for<'a> Fn(&'a Dio) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<CborValue>> + Send + 'static,
{
    Box::new(move |dio| Box::pin(f(dio)))
}

/// What a `ValueScenery` computes. Variants land here as direct cache
/// scans; vista 5b will add push-down via `dio.vista()`.
pub enum Aggregate {
    /// Row count over the entire cache.
    Count,
    /// Row count over rows matching every `(col, value)` equality.
    CountWhere(Vec<(String, CborValue)>),
    /// Sum of the named column (integer fields only in v1).
    Sum(String),
    /// Max of the named column (integer fields only in v1).
    Max(String),
    /// Min of the named column (integer fields only in v1).
    Min(String),
    /// Free-form aggregate — closure receives the Dio and returns
    /// any [`CborValue`]. Use this for join-style or schema-specific
    /// reductions the canned variants don't cover.
    Custom(CustomAggregate),
}

impl std::fmt::Debug for Aggregate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Aggregate::Count => f.write_str("Count"),
            Aggregate::CountWhere(c) => f.debug_tuple("CountWhere").field(c).finish(),
            Aggregate::Sum(s) => f.debug_tuple("Sum").field(s).finish(),
            Aggregate::Max(s) => f.debug_tuple("Max").field(s).finish(),
            Aggregate::Min(s) => f.debug_tuple("Min").field(s).finish(),
            Aggregate::Custom(_) => f.write_str("Custom(<closure>)"),
        }
    }
}

/// Builder produced by [`Dio::value_scenery`](crate::Dio::value_scenery).
pub struct ValueSceneryBuilder {
    pub(crate) dio: Arc<DioInner>,
    pub(crate) aggregate: Option<Aggregate>,
}

impl ValueSceneryBuilder {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        Self {
            dio,
            aggregate: None,
        }
    }

    pub fn aggregate(mut self, agg: Aggregate) -> Self {
        self.aggregate = Some(agg);
        self
    }

    pub fn count(self) -> Self {
        self.aggregate(Aggregate::Count)
    }

    pub fn count_where(self, conds: Vec<(String, CborValue)>) -> Self {
        self.aggregate(Aggregate::CountWhere(conds))
    }

    pub fn sum(self, col: impl Into<String>) -> Self {
        self.aggregate(Aggregate::Sum(col.into()))
    }

    pub fn max(self, col: impl Into<String>) -> Self {
        self.aggregate(Aggregate::Max(col.into()))
    }

    pub fn min(self, col: impl Into<String>) -> Self {
        self.aggregate(Aggregate::Min(col.into()))
    }

    /// Sugar for `aggregate(Aggregate::Custom(boxed_custom_aggregate(f)))`.
    pub fn custom<F, Fut>(self, f: F) -> Self
    where
        F: for<'a> Fn(&'a Dio) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<CborValue>> + Send + 'static,
    {
        self.aggregate(Aggregate::Custom(boxed_custom_aggregate(f)))
    }

    /// Open the Scenery. Performs the initial compute, spawns the
    /// bus reactor, returns the handle.
    pub async fn open(self) -> Result<Arc<dyn ValueScenery>> {
        let aggregate = self
            .aggregate
            .ok_or_else(|| error!("ValueSceneryBuilder needs an aggregate; call .count(), .sum(...), .custom(...), or .aggregate(...) before .open()"))?;

        let (gen_tx, _gen_rx) = watch::channel(Generation::default());
        let state = Arc::new(ValueSceneryState {
            dio_weak: Arc::downgrade(&self.dio),
            aggregate,
            value: RwLock::new(None),
            status: RwLock::new(ValueStatus::Loading),
            generation: AtomicU64::new(0),
            generation_tx: gen_tx,
        });

        state.recompute().await;

        let bus_rx = self.dio.event_bus.subscribe();
        let task_state = state.clone();
        self.dio.lens.runtime.spawn(async move {
            recompute_loop(task_state, bus_rx).await;
        });

        Ok(Arc::new(ValueSceneryImpl { inner: state }) as Arc<dyn ValueScenery>)
    }
}

pub(crate) struct ValueSceneryState {
    pub(crate) dio_weak: Weak<DioInner>,
    pub(crate) aggregate: Aggregate,
    pub(crate) value: RwLock<Option<CborValue>>,
    pub(crate) status: RwLock<ValueStatus>,
    pub(crate) generation: AtomicU64,
    pub(crate) generation_tx: watch::Sender<Generation>,
}

impl ValueSceneryState {
    async fn recompute(&self) {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return;
        };
        let dio = Dio { inner: dio_inner };

        let outcome = dispatch(&self.aggregate, &dio).await;
        let mut changed = false;
        match outcome {
            Ok(new_value) => {
                let mut status = self.status.write().unwrap();
                if !matches!(*status, ValueStatus::Fresh) {
                    *status = ValueStatus::Fresh;
                    changed = true;
                }
                drop(status);

                let mut current = self.value.write().unwrap();
                if current.as_ref() != Some(&new_value) {
                    *current = Some(new_value);
                    changed = true;
                }
            }
            Err(e) => {
                let msg = e.to_string();
                let mut status = self.status.write().unwrap();
                if !matches!(*status, ValueStatus::Error(ref existing) if existing == &msg) {
                    *status = ValueStatus::Error(msg);
                    changed = true;
                }
                // Value preserved.
            }
        }
        if changed {
            self.bump_generation();
        }
    }

    fn bump_generation(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // `send_replace` (not `send`) — the stored value must reflect the
        // current generation even when there are momentarily zero
        // receivers. UIs that drop and re-subscribe must see the latest.
        let _ = self.generation_tx.send_replace(Generation(next));
    }
}

async fn dispatch(agg: &Aggregate, dio: &Dio) -> Result<CborValue> {
    match agg {
        Aggregate::Count => {
            let n = dio.cache().count().await?;
            Ok(CborValue::Integer(n.into()))
        }
        Aggregate::CountWhere(conds) => {
            let rows = dio.cache().list_values().await?;
            let n = rows
                .values()
                .filter(|rec| conds.iter().all(|(col, v)| rec.get(col) == Some(v)))
                .count() as i64;
            Ok(CborValue::Integer(n.into()))
        }
        Aggregate::Sum(field) => scan_sum(dio, field).await,
        Aggregate::Max(field) => scan_extreme(dio, field, true).await,
        Aggregate::Min(field) => scan_extreme(dio, field, false).await,
        Aggregate::Custom(f) => f(dio).await,
    }
}

async fn scan_sum(dio: &Dio, field: &str) -> Result<CborValue> {
    let rows = dio.cache().list_values().await?;
    let mut sum: i128 = 0;
    for (_, rec) in rows {
        match rec.get(field) {
            Some(CborValue::Integer(i)) => sum = sum.saturating_add(i128::from(*i)),
            Some(_) => {
                return Err(error!(
                    "ValueScenery::Sum requires integer fields",
                    field = field.to_string()
                ));
            }
            None => {}
        }
    }
    let as_int: ciborium::value::Integer = sum.try_into().map_err(|_| {
        error!(
            "ValueScenery::Sum overflowed i128 → cbor integer",
            field = field.to_string()
        )
    })?;
    Ok(CborValue::Integer(as_int))
}

async fn scan_extreme(dio: &Dio, field: &str, max: bool) -> Result<CborValue> {
    let rows = dio.cache().list_values().await?;
    let mut acc: Option<i128> = None;
    for (_, rec) in rows {
        match rec.get(field) {
            Some(CborValue::Integer(i)) => {
                let v = i128::from(*i);
                acc = Some(match acc {
                    None => v,
                    Some(cur) if max && v > cur => v,
                    Some(cur) if !max && v < cur => v,
                    Some(cur) => cur,
                });
            }
            Some(_) => {
                return Err(error!(
                    "ValueScenery::Max/Min requires integer fields",
                    field = field.to_string()
                ));
            }
            None => {}
        }
    }
    match acc {
        Some(v) => {
            let as_int: ciborium::value::Integer = v.try_into().map_err(|_| {
                error!(
                    "ValueScenery::Max/Min overflowed i128 → cbor integer",
                    field = field.to_string()
                )
            })?;
            Ok(CborValue::Integer(as_int))
        }
        None => Ok(CborValue::Null),
    }
}

async fn recompute_loop(state: Arc<ValueSceneryState>, mut bus: broadcast::Receiver<DioEvent>) {
    loop {
        if state.dio_weak.upgrade().is_none() {
            return;
        }
        match bus.recv().await {
            Ok(DioEvent::WriteFailed { .. }) => {}
            Ok(_) => state.recompute().await,
            Err(broadcast::error::RecvError::Lagged(_)) => state.recompute().await,
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

pub(crate) struct ValueSceneryImpl {
    pub(crate) inner: Arc<ValueSceneryState>,
}

impl ValueScenery for ValueSceneryImpl {
    fn value(&self) -> Option<CborValue> {
        self.inner.value.read().unwrap().clone()
    }

    fn status(&self) -> ValueStatus {
        self.inner.status.read().unwrap().clone()
    }

    fn request_refresh(&self) {
        let Some(dio_inner) = self.inner.dio_weak.upgrade() else {
            return;
        };
        let runtime = dio_inner.lens.runtime.clone();
        let state = self.inner.clone();
        runtime.spawn(async move {
            let dio = Dio { inner: dio_inner };
            if let Err(e) = dio.refresh().await {
                tracing::error!(error = %e, "ValueScenery request_refresh failed");
            }
            // refresh() emits `DatasetChanged`; the recompute loop picks it up
            // and the value reloads. Trigger an immediate recompute too so
            // a callback-less Lens still sees a fresh value.
            state.recompute().await;
        });
    }

    fn subscribe(&self) -> watch::Receiver<Generation> {
        self.inner.generation_tx.subscribe()
    }
}
