//! `AggregateLens` — the aggregate datasource.
//!
//! A derived Dio's needs are the inverse of a source Dio's: there is no master
//! to fetch from, nothing to write back, and its contents are reproducible from
//! the source at any time. This Lens encodes exactly that, and owns **one cache
//! file for every aggregation** — each aggregate claiming a named table within
//! it, the same shape a datasource uses for its tables.

mod mount;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use vantage_core::{Context as _, Result};
use vantage_diorama::{Cache, CacheBackend, Lens, MemoryCache, RedbCache, Tuning};

/// How long to hold off after reacting, before flushing whatever else arrived.
const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(250);

pub struct AggregateLensBuilder {
    backend: Arc<dyn CacheBackend>,
    debounce: Duration,
}

impl AggregateLensBuilder {
    /// Cooldown after each recomputation.
    ///
    /// The first change in a burst is always acted on immediately; this bounds
    /// how often the ones behind it are. Whatever arrives during the cooldown
    /// is flushed once at the end, never dropped.
    pub fn debounce(mut self, window: Duration) -> Self {
        self.debounce = window;
        self
    }

    /// Build the Lens. Fails outside a tokio runtime.
    pub fn build(self) -> Result<Arc<AggregateLens>> {
        let lens = Lens::builder()
            .cache(Cache::backend(self.backend.clone()))
            // Nothing upstream to re-fetch: this Lens's Dios are fed by the
            // aggregation engine, not by a master.
            .tuning(Tuning::default().with_refresh_on_open(false))
            .build()
            .context(vantage_core::error!("the aggregate lens failed to build"))?;
        Ok(Arc::new(AggregateLens {
            lens,
            backend: self.backend,
            debounce: self.debounce,
        }))
    }
}

pub struct AggregateLens {
    lens: Arc<Lens>,
    backend: Arc<dyn CacheBackend>,
    debounce: Duration,
}

impl AggregateLens {
    /// Persist derived results at `path` — one file, one table per aggregate.
    pub fn at(path: impl AsRef<Path>) -> Result<AggregateLensBuilder> {
        Ok(AggregateLensBuilder {
            backend: Arc::new(RedbCache::open(path)?),
            debounce: DEFAULT_DEBOUNCE,
        })
    }

    /// Keep derived results in memory only. Aggregates are reproducible from
    /// their source, so this loses nothing but the warm start.
    pub fn in_memory() -> AggregateLensBuilder {
        AggregateLensBuilder {
            backend: Arc::new(MemoryCache::new()),
            debounce: DEFAULT_DEBOUNCE,
        }
    }

    /// Reuse a backend already opened elsewhere — e.g. to put aggregates in the
    /// same file as the datasource they derive from.
    pub fn with_backend(backend: Arc<dyn CacheBackend>) -> AggregateLensBuilder {
        AggregateLensBuilder {
            backend,
            debounce: DEFAULT_DEBOUNCE,
        }
    }

    /// Close the aggregate cache cleanly, before the process exits.
    ///
    /// Derived results are reproducible, so losing this file costs only a warm
    /// start — but an unclean close costs a full-file repair scan on every
    /// launch afterwards, which is worse than the thing the cache buys.
    pub async fn close(&self) {
        self.backend.close().await;
    }

    /// Drop cached aggregates whose names are no longer in use.
    ///
    /// Names encode what an aggregate *is*; when a definition changes, the old
    /// table would otherwise sit there forever. A backend that can't enumerate
    /// its tables returns an error.
    pub async fn retain(&self, keep: &[&str]) -> Result<()> {
        for name in self.backend.list_tables().await? {
            if keep.contains(&name.as_str()) {
                continue;
            }
            tracing::debug!(
                target: "vantage_diorama_aggregate",
                table = %name,
                "dropping unused aggregate cache",
            );
            self.backend.drop_table(&name).await?;
        }
        Ok(())
    }
}
