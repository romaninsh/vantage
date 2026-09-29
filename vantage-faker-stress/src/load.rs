//! Consumers for each table's events. Without a Dio a subscriber only
//! counts; with one it applies every event to a Dio and keeps a sorted
//! TableScenery open, which is the work a vantage-ui grid does.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::broadcast::{self, error::RecvError};
use tokio::task::JoinHandle;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{ChangeEvent, Lens, SortDir, TableScenery};
use vantage_faker::FakerHandle;
use vantage_vista::Vista;

use crate::sampler::EventTotals;

/// Rows a scenery keeps materialized, like a grid's visible page.
const VIEWPORT: usize = 50;

pub struct TableLoad {
    delivered: Arc<AtomicU64>,
    lagged: Arc<AtomicU64>,
    events: broadcast::Sender<ChangeEvent>,
    scenery: Option<Arc<dyn TableScenery>>,
    task: JoinHandle<()>,
}

impl TableLoad {
    pub fn totals(&self) -> EventTotals {
        EventTotals {
            delivered: self.delivered.load(Ordering::Relaxed),
            lagged: self.lagged.load(Ordering::Relaxed),
            backlog: self.events.len(),
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
        })
}

/// A lens whose Dios warm their cache from the master at start, then apply
/// faker events in place, as vantage-ui's do.
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
        .on_event(|dio, evt| {
            let dio = dio.clone();
            async move {
                match evt {
                    ChangeEvent::Inserted {
                        id,
                        new: Some(record),
                    }
                    | ChangeEvent::Updated {
                        id,
                        new: Some(record),
                    } => dio.patched(id, record).await?,
                    ChangeEvent::Deleted { id } => dio.removed(id).await?,
                    ChangeEvent::Invalidated => {
                        dio.cache().clear().await?;
                        dio.notify_dataset_changed();
                    }
                    _ => {}
                }
                Ok(())
            }
        })
        .build()
        .map_err(|e| format!("lens: {e}"))?;
    Ok(Arc::new(lens))
}

/// Subscribe to `handle`'s events. With a `lens`, `vista` becomes a Dio and
/// every event is applied to it.
pub async fn attach(
    vista: Vista,
    handle: &FakerHandle,
    lens: Option<&Arc<Lens>>,
) -> Result<TableLoad, String> {
    let mut rx = handle.events.subscribe();
    let delivered = Arc::new(AtomicU64::new(0));
    let lagged = Arc::new(AtomicU64::new(0));
    let (dio, scenery) = match lens {
        Some(lens) => {
            let dio = lens
                .make_dio(vista)
                .await
                .map_err(|e| format!("dio: {e}"))?;
            let scenery: Arc<dyn TableScenery> = dio
                .table_scenery()
                .sort("id", SortDir::Asc)
                .open()
                .await
                .map_err(|e| format!("scenery: {e}"))?;
            scenery.set_viewport(0..VIEWPORT);
            (Some(dio), Some(scenery))
        }
        None => (None, None),
    };
    let (d, l) = (delivered.clone(), lagged.clone());
    let task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(evt) => {
                    if let Some(dio) = &dio {
                        let _ = dio.handle_event(evt).await;
                    }
                    d.fetch_add(1, Ordering::Relaxed);
                }
                Err(RecvError::Lagged(n)) => {
                    l.fetch_add(n, Ordering::Relaxed);
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
    Ok(TableLoad {
        delivered,
        lagged,
        events: handle.events.clone(),
        scenery,
        task,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use vantage_faker::{FakerColumn, FakerTable, StaticEffect};

    fn table() -> FakerTable {
        let mut id = FakerColumn::new("id", "string");
        id.flags.push("id".into());
        FakerTable::build(
            "t",
            vec![id, FakerColumn::new("note", "string")],
            "id",
            Box::new(StaticEffect { count: 5 }),
        )
    }

    async fn push_three(handle: &vantage_faker::FakerHandle) {
        for _ in 0..3 {
            handle.ctx().push();
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn counting_subscriber_sees_every_event() {
        let (vista, handle) = table().split();
        let load = attach(vista, &handle, None).await.unwrap();
        push_three(&handle).await;
        let t = load.totals();
        assert_eq!(t.delivered, 3);
        assert_eq!(t.backlog, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dio_subscriber_applies_events() {
        let dir = tempfile::tempdir().unwrap();
        let lens = lens(dir.path()).unwrap();
        let (vista, handle) = table().split();
        let load = attach(vista, &handle, Some(&lens)).await.unwrap();
        push_three(&handle).await;
        assert_eq!(load.totals().delivered, 3);
        assert_eq!(load.scenery_rows(), Some(8));
    }
}
