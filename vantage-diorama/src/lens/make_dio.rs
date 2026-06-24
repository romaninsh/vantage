//! `Lens::make_dio` — the entry point that binds a master Vista to the
//! Lens's cache + callbacks and produces a [`Dio`].
//!
//! Spawns the per-Dio write worker (always) and the refresh task (when
//! `refresh_every` and `on_refresh` are both set). Fires `on_start`
//! either blocking or detached per
//! [`LensDefaults::on_start_blocking`](crate::lens::LensDefaults::on_start_blocking).

use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::{Mutex, broadcast, mpsc};
use vantage_core::Result;
use vantage_vista::Vista;

use crate::dio::{Dio, DioEvent, DioInner, HotTier, worker::write_worker_loop};
use crate::lens::Lens;

impl Lens {
    /// Bind `master` to this Lens. Opens the cache table, spawns the
    /// write worker (+ refresh task if configured), fires `on_start`,
    /// and returns the live [`Dio`].
    pub async fn make_dio(self: &Arc<Self>, master: Vista) -> Result<Dio> {
        let cache_table_name = master.name().to_string();
        let cache = self.cache_source.open_table(&cache_table_name).await?;

        let (write_tx, write_rx) = mpsc::channel(self.defaults.write_queue_capacity);
        let (event_bus, _event_rx) = broadcast::channel(64);

        let inner = Arc::new(DioInner {
            lens: self.clone(),
            master: std::sync::RwLock::new(Arc::new(master)),
            cache,
            cache_table_name,
            write_queue: write_tx,
            event_bus,
            refresh_task: Mutex::new(None),
            write_worker: Mutex::new(None),
            hot_tier: Arc::new(HotTier::new()),
            query_indexes: std::sync::Mutex::new(std::collections::HashMap::new()),
            table_sceneries: std::sync::Mutex::new(std::collections::HashMap::new()),
        });
        let dio = Dio { inner };

        spawn_write_worker(&dio, write_rx).await;
        spawn_refresh_task(&dio).await;

        if let Some(on_start) = self.callbacks.on_start.as_ref() {
            if self.defaults.on_start_blocking {
                on_start(&dio).await?;
            } else {
                let dio_for_task = dio.clone();
                let lens_for_task = self.clone();
                self.runtime.spawn(async move {
                    if let Some(cb) = lens_for_task.callbacks.on_start.as_ref()
                        && let Err(e) = cb(&dio_for_task).await
                    {
                        tracing::error!(error = %e, "on_start callback failed");
                    }
                });
            }
        }

        Ok(dio)
    }
}

async fn spawn_write_worker(dio: &Dio, rx: mpsc::Receiver<crate::ops::WriteOp>) {
    let inner_weak = Arc::downgrade(&dio.inner);
    let handle = dio.inner.lens.runtime.spawn(async move {
        write_worker_loop(inner_weak, rx).await;
    });
    *dio.inner.write_worker.lock().await = Some(handle);
}

async fn spawn_refresh_task(dio: &Dio) {
    let Some(interval) = dio.inner.lens.defaults.refresh_interval else {
        return;
    };
    if dio.inner.lens.callbacks.on_refresh.is_none() {
        return;
    }
    let inner_weak = Arc::downgrade(&dio.inner);
    let handle = dio.inner.lens.runtime.spawn(async move {
        refresh_loop(inner_weak, interval).await;
    });
    *dio.inner.refresh_task.lock().await = Some(handle);
}

async fn refresh_loop(inner: Weak<DioInner>, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);
    // Skip the immediate tick — `on_start` typically just ran and refresh
    // should fire after `interval`, not at t=0.
    ticker.tick().await;
    loop {
        ticker.tick().await;
        let Some(strong) = inner.upgrade() else {
            return;
        };
        let dio = Dio { inner: strong };
        let _ = dio.inner.event_bus.send(DioEvent::Refreshing);
        if let Some(cb) = dio.inner.lens.callbacks.on_refresh.as_ref()
            && let Err(e) = cb(&dio).await
        {
            tracing::error!(error = %e, "on_refresh callback failed");
        }
        let _ = dio.inner.event_bus.send(DioEvent::Invalidated);
    }
}
