//! Spawning a built Dio: its state, its write worker, its refresh timer, its
//! eager start and its live follow.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::{Mutex, broadcast, mpsc};
use vantage_core::Result;

use super::registry::DioKey;
use super::{Lens, LoaderCx};
use crate::dio::{Dio, DioInner, DioParts, Follower, worker::write_worker_loop};

/// Events a Dio's bus holds before a slow subscriber lags.
pub(crate) const EVENT_BUS_CAPACITY: usize = 64;

impl Lens {
    /// Open the cache table, spawn the write worker and the refresh timer,
    /// run the eager start when the loader has one for this master, request
    /// a background refresh when the cache is older than the refresher's
    /// `max_age`, and start the live follow when asked for. Registered under
    /// `key` once it is up; a failed spawn registers nothing.
    pub(crate) async fn spawn_dio(self: &Arc<Self>, key: DioKey, parts: DioParts) -> Result<Dio> {
        let DioParts {
            master,
            residual,
            query,
            metadata,
            id_column,
            augmentations,
            augmented_columns,
            live,
            cache_table_name,
        } = parts;
        let cache = self.cache.open_table(&cache_table_name).await?;
        let loader = &self.callbacks.loader;
        let eager = loader.runs_eager_start(master.capabilities());
        let detached = eager && !loader.eager_blocking();

        let (write_tx, write_rx) = mpsc::channel(self.tuning().write_queue);
        let (event_bus, _event_rx) = broadcast::channel(EVENT_BUS_CAPACITY);

        let inner = Arc::new(DioInner {
            _tally: crate::stats::Tally::dio(),
            lens: self.clone(),
            master,
            residual,
            query,
            metadata,
            id_column,
            live,
            cache,
            cache_table_name,
            write_queue: write_tx,
            event_bus,
            pending_flashes: Arc::new(crate::dio::pending::PendingFlashes::new()),
            refresh_task: Mutex::new(None),
            write_worker: Mutex::new(None),
            query_indexes: std::sync::Mutex::new(std::collections::HashMap::new()),
            shared_rows: std::sync::Mutex::new(std::collections::HashMap::new()),
            table_views: std::sync::Mutex::new(Vec::new()),
            augmentations,
            augmented_columns,
            augment_scheduler: Arc::new(crate::dio::augment_scheduler::AugmentScheduler::new()),
            augment_worker_handles: std::sync::Mutex::new(Vec::new()),
            detail_state: std::sync::Mutex::new(std::collections::HashMap::new()),
            // Set before any scenery can open, so one opening over the cold
            // cache waits for the detached start instead of showing no rows.
            seed_complete: AtomicBool::new(!detached),
            whole_copy: std::sync::Mutex::new(None),
            built_at: std::time::SystemTime::now(),
            stale_check_pending: AtomicBool::new(false),
            req_seq: std::sync::atomic::AtomicU64::new(0),
            servo_census: std::sync::atomic::AtomicUsize::new(0),
            record_census: std::sync::atomic::AtomicUsize::new(0),
            watch_cancel: tokio::sync::watch::Sender::new(()),
        });
        let dio = Dio { inner };
        announce(&dio).await;

        spawn_write_worker(&dio, write_rx).await;
        spawn_refresh_timer(&dio).await;
        if dio.inner.has_dio_augment() {
            dio.inner.ensure_augment_workers();
        }

        // Subscribed before the eager start copies the set and followed once
        // the copy has landed, so a change made during the copy is applied
        // over it rather than lost or overwritten by it.
        let follower = if dio.inner.live {
            dio.subscribe().await?
        } else {
            None
        };
        if eager {
            run_eager_start(&dio, detached, follower).await?;
        } else {
            // An eager start is itself the fresh load; anything else may be
            // serving a cache older than the refresher allows.
            dio.inner.refresh_if_stale();
            spawn_follow(&dio, follower);
        }
        Ok(self.dios.insert(key, &dio))
    }
}

fn spawn_follow(dio: &Dio, follower: Option<Follower>) {
    if let Some(follower) = follower {
        dio.inner.lens.runtime.spawn(follower.run());
    }
}

/// The debug stream's first line for a Dio: what it is a copy of, and whether
/// it starts from anything.
async fn announce(dio: &Dio) {
    let tap = dio.inner.tap().clone();
    if !tap.enabled() {
        return;
    }
    let rows = dio.inner.cache.count().await.unwrap_or(0).max(0) as usize;
    crate::debug::tapline!(
        tap,
        "dio",
        "created over \"{}\" — cache table \"{}\", {}",
        dio.inner.master.name(),
        dio.inner.cache_table_name,
        if rows == 0 {
            "nothing held yet".to_string()
        } else {
            format!(
                "{} rows already held from a previous session",
                crate::debug::num(rows)
            )
        },
    );
}

/// Run the loader's eager start, awaited or detached, then start `follower`.
/// A detached start announces its rows with `Seeded` once they land, because
/// writes through the raw cache raise no events of their own.
async fn run_eager_start(dio: &Dio, detached: bool, follower: Option<Follower>) -> Result<()> {
    if !detached {
        start(dio).await?;
        dio.inner.stamp_whole().await;
        spawn_follow(dio, follower);
        return Ok(());
    }
    let dio = dio.clone();
    let runtime = dio.inner.lens.runtime.clone();
    runtime.spawn(async move {
        let outcome = start(&dio).await;
        // Set before the event: `Seeded` wakes the sceneries, and they read
        // this flag to decide whether an empty cache is an answer. A failed
        // start is one that is no longer coming, too.
        dio.inner.seed_complete.store(true, Ordering::SeqCst);
        match outcome {
            Ok(()) => {
                dio.inner.stamp_whole().await;
                dio.notify_seeded();
            }
            Err(e) => tracing::error!(error = %e, "eager start failed"),
        }
        spawn_follow(&dio, follower);
    });
    Ok(())
}

async fn start(dio: &Dio) -> Result<()> {
    match dio.inner.lens.callbacks.loader.eager.as_ref() {
        Some(start) => start(LoaderCx::new(dio.clone())).await,
        None => Ok(()),
    }
}

async fn spawn_write_worker(dio: &Dio, rx: mpsc::Receiver<crate::dio::worker::QueuedFlash>) {
    let handle = dio.inner.lens.runtime.spawn(async move {
        write_worker_loop(rx).await;
    });
    *dio.inner.write_worker.lock().await = Some(handle);
}

/// The timer runs only when the Lens's refresher has an interval.
async fn spawn_refresh_timer(dio: &Dio) {
    let Some(every) = dio
        .inner
        .lens
        .callbacks
        .refresher
        .as_ref()
        .and_then(|r| r.every)
    else {
        return;
    };
    let inner_weak = Arc::downgrade(&dio.inner);
    let handle = dio.inner.lens.runtime.spawn(async move {
        refresh_loop(inner_weak, every).await;
    });
    *dio.inner.refresh_task.lock().await = Some(handle);
}

/// Waits `every`, then refreshes, until the Dio drops. Sleeping before the
/// first tick skips a refresh right after the build loaded the cache.
///
/// Each tick takes the scheduled path, which leaves augmentation rows known
/// to have no detail record alone; re-probing them every tick would keep the
/// detail sweep from ever converging.
async fn refresh_loop(inner: Weak<DioInner>, every: Duration) {
    loop {
        tokio::time::sleep(every).await;
        let Some(strong) = inner.upgrade() else {
            return;
        };
        let dio = Dio { inner: strong };
        if let Err(e) = dio.refresh_scheduled().await {
            tracing::error!(error = %e, "scheduled refresh failed");
        }
    }
}
