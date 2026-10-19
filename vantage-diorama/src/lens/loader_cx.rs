//! [`LoaderCx`]: what a Lens callback is handed about the Dio it serves.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{Vista, VistaQuery};

use super::cache_backend::CacheTable;
use crate::dio::pending::SettleMark;
use crate::dio::{Dio, Fetched};

/// The Dio a loader, refresher or writer is serving: its master and its
/// cache table. Owned and cheap to clone, so a callback's future can hold it
/// across awaits.
#[derive(Clone)]
pub struct LoaderCx {
    dio: Dio,
    /// Taken when the callback is handed this context, before it lists
    /// anything: a reconcile skips rows a flash settled since.
    mark: Arc<SettleMark>,
    /// Set once the callback reconciled, which already recorded the refresh
    /// and announced it.
    reconciled: Arc<AtomicBool>,
}

impl LoaderCx {
    pub(crate) fn new(dio: Dio) -> Self {
        let mark = Arc::new(dio.inner.pending_flashes.mark());
        Self {
            dio,
            mark,
            reconciled: Arc::default(),
        }
    }

    /// The Dio's master Vista, as narrowed for this Dio.
    pub fn master(&self) -> &Vista {
        self.dio.master()
    }

    /// The Dio's cache table. Writes here raise no events, ignore flashes
    /// in flight, and stay unknown to the other Dios over the table;
    /// [`reconcile`](Self::reconcile) is the write for a full set.
    pub fn cache(&self) -> &Arc<dyn CacheTable> {
        &self.dio.inner.cache
    }

    /// Make the cache hold exactly `snapshot`, the whole set: see
    /// [`Dio::reconcile`]. Rows with a flash in flight, or whose flash
    /// settled since this callback began, are left alone; the refresh time
    /// is recorded and one `DatasetChanged` is published.
    pub async fn reconcile(&self, snapshot: IndexMap<String, Record<CborValue>>) -> Result<()> {
        self.dio.reconcile_since(&self.mark, snapshot).await?;
        self.reconciled.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Read `window` of the master narrowed by `query`: the view query a
    /// paged loader is handed. See [`Dio::fetch`].
    pub async fn fetch(&self, window: Range<usize>, query: &VistaQuery) -> Result<Fetched> {
        self.dio.fetch(window, query).await
    }

    /// Whether the callback reconciled through this context.
    pub(crate) fn reconciled(&self) -> bool {
        self.reconciled.load(Ordering::SeqCst)
    }
}
