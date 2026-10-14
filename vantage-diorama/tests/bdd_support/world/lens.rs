//! Materialising the configured Lens: loaders, refresher and writer.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{
    Cache, CacheBackend, ChangeFlash, FlashKind, FlashOutcome, Lens, Loader, LoaderCx, Refresher,
    Tuning,
};
use vantage_vista::Vista;

use super::config::{LensBuilderState, OnFlashMode, OnLoadChunkKind, OnStartLoad};
use super::on_master;
use crate::bdd_support::backend::BackendKind;
use crate::bdd_support::spies::Spies;

/// Send one flash to the master: the write-through a mirroring writer does.
/// The Dio stages the change in the cache itself and keeps it on success.
async fn write_master(master: Vista, flash: ChangeFlash) -> Result<()> {
    use vantage_dataset::traits::WritableValueSet;
    let id = || flash.id().expect("the flash carries an id").to_string();
    match flash.kind() {
        FlashKind::Insert => drop(master.insert_value(id(), flash.patch()).await?),
        FlashKind::Replace => drop(master.replace_value(id(), flash.patch()).await?),
        FlashKind::Upsert => drop(master.upsert_value(&id(), flash.patch()).await?),
        FlashKind::Patch => drop(master.patch_value(id(), flash.patch()).await?),
        FlashKind::Delete => master.delete(id()).await?,
        FlashKind::Clear => master.delete_all().await?,
        other => {
            return Err(vantage_core::error!(
                "the mirroring writer has no route for this flash kind",
                kind = format!("{other:?}")
            ));
        }
    }
    Ok(())
}

impl LensBuilderState {
    /// Materialise the configured Lens. Closures clone the spy counters so
    /// each callback invocation lands in the matching `AtomicU64`.
    ///
    /// The scenario's hooks map onto the 2.0 Lens as follows:
    /// - a chunk callback, or a total provider alone, becomes
    ///   `Loader::paged`. A paged Lens copies nothing at build over a master
    ///   with `Window`, so its "on_start" seed is written by
    ///   `DioramaWorld::seed_paged_start` before the Dio is built, and the
    ///   total provider's count is stored as the cache's remembered total
    ///   there and stated by each chunk load;
    /// - otherwise "on_start" is `Loader::eager`, with `on_start_blocking`
    ///   as `.blocking(..)`;
    /// - "on_refresh" and the refresh interval are a `Refresher`;
    /// - "on_flash" is the Lens writer;
    /// - `refresh_on_open` is a `Tuning` setting.
    pub fn build(
        &self,
        cache: Arc<dyn CacheBackend>,
        spies: &Spies,
        backend: BackendKind,
    ) -> Result<Arc<Lens>> {
        let bridge = backend == BackendKind::Sqlite;
        let mut b = Lens::builder().cache(Cache::backend(cache));

        let load_kind = self.load_kind();
        if self.pages() {
            b = b.loader(self.paged_loader(spies, bridge));
        } else if !matches!(load_kind, OnStartLoad::Unset) {
            let counter = spies.on_start.clone();
            let master_list_counter = spies.master_list_calls.clone();
            let gate = self.on_start_gate.clone();
            let mut loader = Loader::eager(move |cx: LoaderCx| {
                let counter = counter.clone();
                let master_list_counter = master_list_counter.clone();
                let gate = gate.clone();
                async move {
                    if let Some(n) = gate.as_ref() {
                        n.notified().await;
                    }
                    counter.fetch_add(1, Ordering::SeqCst);
                    master_list_counter.fetch_add(1, Ordering::SeqCst);
                    let rows = on_master(bridge, cx.master().clone(), |m| async move {
                        m.list_values().await
                    })
                    .await?;
                    cx.cache().insert_values(load_kind.select(rows)).await
                }
            });
            if let Some(blocking) = self.on_start_blocking {
                loader = loader.blocking(blocking);
            }
            b = b.loader(loader);
        }

        let refresher = if self.register_on_refresh {
            let counter = spies.on_refresh.clone();
            let fails = self.on_refresh_fails;
            Some(Refresher::new(move |_cx: LoaderCx| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    if fails {
                        return Err(vantage_core::error!("on_refresh failed (injected 503)"));
                    }
                    Ok(())
                }
            }))
        } else {
            self.refresh_every
                .map(|_| Refresher::new(|_cx: LoaderCx| async { Ok(()) }))
        };
        if let Some(mut refresher) = refresher {
            if let Some(interval) = self.refresh_every {
                refresher = refresher.every(interval);
            }
            b = b.refresher(refresher);
        }

        if !matches!(self.on_flash_mode, OnFlashMode::Unset) {
            let mode = self.on_flash_mode;
            let counter = spies.on_flash.clone();
            b = b.writer(move |cx: LoaderCx, flash: ChangeFlash| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    match mode {
                        OnFlashMode::Error => {
                            return Err(vantage_core::error!("on_flash rejected"));
                        }
                        OnFlashMode::Mirror => {
                            on_master(bridge, cx.master().clone(), |m| write_master(m, flash))
                                .await?
                        }
                        OnFlashMode::Pass | OnFlashMode::Unset => {}
                    }
                    Ok(FlashOutcome::applied())
                }
            });
        }

        if let Some(enabled) = self.refresh_on_open {
            b = b.tuning(Tuning::default().with_refresh_on_open(enabled));
        }

        b.build()
    }

    /// The paged loader for the configured chunk callback (a total provider
    /// alone pulls from the master). With a total provider, every chunk load
    /// also states the master's count.
    fn paged_loader(&self, spies: &Spies, bridge: bool) -> Loader {
        let kind = match self.on_load_chunk_kind {
            OnLoadChunkKind::Unset => OnLoadChunkKind::PullFromMaster,
            other => other,
        };
        let total_kind = self.total_provider_kind;
        let spies = spies.clone();
        Loader::paged(move |cx: LoaderCx, range, _query, sink| {
            let spies = spies.clone();
            async move {
                spies.on_load_chunk.fetch_add(1, Ordering::SeqCst);
                *spies.last_load_chunk_range.lock().await = Some(range.clone());
                match kind {
                    OnLoadChunkKind::RecordCalls => return Ok(()),
                    OnLoadChunkKind::AlwaysError => {
                        return Err(vantage_core::error!("on_load_chunk rejected"));
                    }
                    OnLoadChunkKind::PullFromMaster | OnLoadChunkKind::Unset => {}
                }
                // Scriptable-source gate: virtual-time latency, then a
                // self-clearing counted fault. Both default to 0, so an
                // unscripted scenario is unaffected.
                let latency_ms = spies.source_latency_ms.load(Ordering::SeqCst);
                if latency_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(latency_ms)).await;
                }
                if spies.source_fail_reads.load(Ordering::SeqCst) > 0 {
                    spies.source_fail_reads.fetch_sub(1, Ordering::SeqCst);
                    return Err(vantage_core::error!("source read failed (injected)"));
                }
                if spies.on_load_chunk_error_once.swap(false, Ordering::SeqCst) {
                    return Err(vantage_core::error!("on_load_chunk rejected (one-shot)"));
                }
                spies.master_list_calls.fetch_add(1, Ordering::SeqCst);
                let all = on_master(bridge, cx.master().clone(), |m| async move {
                    m.list_values().await
                })
                .await?;
                if let Some(total) = total_kind.count(&spies, all.len()) {
                    sink.set_total(total);
                }
                let slice: Vec<_> = all.into_iter().collect();
                let start = range.start.min(slice.len());
                let end = range.end.min(slice.len());
                for (offset, (id, rec)) in slice[start..end].iter().enumerate() {
                    sink.push(start + offset, id.clone(), rec.clone()).await?;
                }
                Ok(())
            }
        })
    }
}
