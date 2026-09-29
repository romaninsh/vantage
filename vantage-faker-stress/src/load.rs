//! Consumers for each store table's changes. A subscriber always counts
//! them; with a lens the table also backs a watching Dio with a sorted
//! TableScenery open, which is the work a vantage-ui grid does.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::broadcast::{Receiver, error::RecvError};
use tokio::task::JoinHandle;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Dio, Lens, SortDir, TableScenery};
use vantage_memory::vista::Catalog;
use vantage_memory::{MemoryChange, MemoryStore, MemoryTableHandle, MemoryTableShell};
use vantage_vista::{Vista, VistaMetadata};

use crate::sampler::EventTotals;

mod drain;
use drain::Drain;

/// Rows a scenery keeps materialized, like a grid's visible page.
const VIEWPORT: usize = 50;

pub struct TableLoad {
    delivered: Arc<AtomicU64>,
    lagged: Arc<AtomicU64>,
    drain: Arc<Drain>,
    /// Subscribed with the counting subscriber and never read, so its
    /// `len()` is every change sent since; minus what the subscriber has
    /// seen, that is the subscriber's backlog.
    sent: Receiver<MemoryChange>,
    /// Keeps the Dio (and so its watch task) alive for the run.
    _dio: Option<Dio>,
    scenery: Option<Arc<dyn TableScenery>>,
    task: JoinHandle<()>,
}

impl TableLoad {
    /// Counters plus a drain-probe reading; call once per sample tick.
    pub fn totals(&self) -> EventTotals {
        let seen = || self.delivered.load(Ordering::Relaxed) + self.lagged.load(Ordering::Relaxed);
        let (lag_ms, backlog) = self
            .drain
            .tick(|| (self.sent.len() as u64).saturating_sub(seen()) as usize);
        EventTotals {
            delivered: self.delivered.load(Ordering::Relaxed),
            lagged: self.lagged.load(Ordering::Relaxed),
            backlog,
            lag_ms,
        }
    }

    /// Rows the scenery holds, when a Dio is attached.
    pub fn scenery_rows(&self) -> Option<usize> {
        self.scenery.as_ref().map(|s| s.row_count())
    }
}

impl Drop for TableLoad {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn sum(loads: &[TableLoad]) -> EventTotals {
    loads
        .iter()
        .map(TableLoad::totals)
        .fold(EventTotals::default(), |a, b| EventTotals {
            delivered: a.delivered + b.delivered,
            lagged: a.lagged + b.lagged,
            backlog: a.backlog + b.backlog,
            lag_ms: a.lag_ms.max(b.lag_ms),
        })
}

/// A lens whose Dios warm their cache from the master at start; `watch`
/// keeps them live from there.
pub fn lens(cache_dir: &Path) -> Result<Arc<Lens>, String> {
    let lens = Lens::new()
        .cache_at(cache_dir.join("cache.redb"))
        .on_start(|dio| {
            let dio = dio.clone();
            async move {
                let rows = dio.master().list_values().await?;
                dio.cache().insert_values(rows).await?;
                Ok(())
            }
        })
        .build()
        .map_err(|e| format!("lens: {e}"))?;
    Ok(Arc::new(lens))
}

/// Count `table`'s changes. With a `lens`, the table also backs a Dio that
/// watches it.
pub async fn attach(
    table: MemoryTableHandle,
    metadata: VistaMetadata,
    store: &MemoryStore,
    lens: Option<&Arc<Lens>>,
) -> Result<TableLoad, String> {
    let mut rx = table.subscribe();
    let sent = table.subscribe();
    let delivered = Arc::new(AtomicU64::new(0));
    let lagged = Arc::new(AtomicU64::new(0));
    let (dio, scenery) = match lens {
        Some(lens) => {
            let name = table.name().to_string();
            let id = table.id_column().to_string();
            let shell = MemoryTableShell::new(table, metadata, Catalog::new(store.clone()));
            let vista = Vista::new(name, Box::new(shell));
            let dio = lens
                .make_dio(vista)
                .await
                .map_err(|e| format!("dio: {e}"))?;
            dio.watch().await.map_err(|e| format!("watch: {e}"))?;
            let scenery: Arc<dyn TableScenery> = dio
                .table_scenery()
                .sort(&id, SortDir::Asc)
                .open()
                .await
                .map_err(|e| format!("scenery: {e}"))?;
            scenery.set_viewport(0..VIEWPORT);
            (Some(dio), Some(scenery))
        }
        None => (None, None),
    };
    let drain = Arc::new(Drain::default());
    let (d, l, dr) = (delivered.clone(), lagged.clone(), drain.clone());
    let task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(_) => {
                    d.fetch_add(1, Ordering::Relaxed);
                    dr.advance(1);
                }
                Err(RecvError::Lagged(n)) => {
                    l.fetch_add(n, Ordering::Relaxed);
                    dr.advance(n);
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
    Ok(TableLoad {
        delivered,
        lagged,
        drain,
        sent,
        _dio: dio,
        scenery,
        task,
    })
}

#[cfg(test)]
mod tests;
