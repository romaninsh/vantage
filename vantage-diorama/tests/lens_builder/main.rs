//! `Lens::builder()`: every setting it refuses, and the refresh timing it
//! sets up — the `every` timer and the `max_age` check on build and open.

mod max_age;
mod refresher;
mod refusals;
#[path = "../support/mod.rs"]
mod support;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::mpsc;
use vantage_core::Result;
use vantage_diorama::{Cache, CacheBackend, Dio, Lens, MemoryCache, Refresher};

use support::teams_master;

const TTL: Duration = Duration::from_secs(60);

// ---- refresh timing ------------------------------------------------------

/// A Lens over `backend` whose refresher reports each run on the channel.
fn counting_lens(backend: Arc<MemoryCache>, refresher: Refresher) -> Arc<Lens> {
    Lens::builder()
        .cache(Cache::backend(backend))
        .refresher(refresher)
        .build()
        .expect("lens builds")
}

fn counting_refresher() -> (Refresher, mpsc::UnboundedReceiver<()>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let refresher = Refresher::new(move |_cx| {
        let tx = tx.clone();
        async move {
            let _ = tx.send(());
            Ok(())
        }
    });
    (refresher, rx)
}

/// Whether a refresh runs within `wait`. Under a paused clock the wait
/// elapses as soon as every task is idle.
async fn refreshed(rx: &mut mpsc::UnboundedReceiver<()>, wait: Duration) -> bool {
    tokio::time::timeout(wait, rx.recv()).await.is_ok()
}

async fn stamp(backend: &MemoryCache, age: Duration) -> Result<()> {
    let table = backend.open_table("members").await?;
    table.set_meta_refreshed_at(SystemTime::now() - age).await
}

/// When the `members` table in `backend` last recorded a refresh.
async fn refreshed_at(backend: &MemoryCache) -> Result<Option<SystemTime>> {
    backend
        .open_table("members")
        .await?
        .meta_refreshed_at()
        .await
}

async fn build(lens: &Arc<Lens>) -> Dio {
    lens.dio(teams_master()).build().await.expect("dio builds")
}
