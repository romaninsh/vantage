use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, RwLock};

use ciborium::Value as CborValue;
use tokio::sync::{Notify, mpsc, watch};
use vantage_core::Result;

use crate::dio::{Dio, DioInner, Generation};

use super::loader::{enqueue_viewport, viewport_loop};
use super::reactor::reload_loop;
use super::state::TableSceneryState;
use super::{SortDir, TableScenery, TableSceneryImpl, ViewportRequest};

/// Builder produced by [`Dio::table_scenery`](crate::Dio::table_scenery).
pub struct TableSceneryBuilder {
    pub(crate) dio: Arc<DioInner>,
    pub(crate) conditions: Vec<(String, CborValue)>,
    pub(crate) sort: Option<(String, SortDir)>,
    pub(crate) search: Option<String>,
    pub(crate) page_size: usize,
    pub(crate) eager: bool,
    pub(crate) initial_range: Option<std::ops::Range<usize>>,
}

impl TableSceneryBuilder {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        Self {
            dio,
            conditions: Vec::new(),
            sort: None,
            search: None,
            page_size: 100,
            eager: false,
            initial_range: None,
        }
    }

    pub fn where_eq(mut self, col: impl Into<String>, value: impl Into<CborValue>) -> Self {
        self.conditions.push((col.into(), value.into()));
        self
    }

    pub fn sort(mut self, col: impl Into<String>, dir: SortDir) -> Self {
        self.sort = Some((col.into(), dir));
        self
    }

    pub fn search(mut self, q: impl Into<String>) -> Self {
        self.search = Some(q.into());
        self
    }

    /// Hint range used by `request_load_more` and by the
    /// refresh-on-open initial fetch. Default 100.
    pub fn page_size(mut self, n: usize) -> Self {
        self.page_size = n;
        self
    }

    /// Currently equivalent to the default — kept so caller code can
    /// continue to target the v1 API shape.
    pub fn eager(mut self) -> Self {
        self.eager = true;
        self
    }

    /// Override the initial range fetched at open time when the
    /// lens's `refresh_on_open` is enabled. Default `0..page_size`.
    pub fn initial_range(mut self, range: std::ops::Range<usize>) -> Self {
        self.initial_range = Some(range);
        self
    }

    /// Open the Scenery — runs `total_provider` (if configured),
    /// seeds the sparse map from the cache, spawns the reactor and
    /// viewport-debounce tasks, optionally schedules a background
    /// initial-load. Returns the live handle.
    pub async fn open(self) -> Result<Arc<dyn TableScenery>> {
        let TableSceneryBuilder {
            dio,
            conditions,
            sort,
            search,
            page_size,
            eager: _,
            initial_range,
        } = self;

        let (gen_tx, _gen_rx) = watch::channel(Generation::default());
        let (viewport_tx, viewport_rx) = mpsc::unbounded_channel();

        let state = Arc::new(TableSceneryState {
            dio_weak: Arc::downgrade(&dio),
            conditions: RwLock::new(conditions),
            sort: RwLock::new(sort),
            search: RwLock::new(search),
            rows: RwLock::new(Default::default()),
            id_to_idx: RwLock::new(HashMap::new()),
            total: RwLock::new(None),
            page_size,
            generation: AtomicU64::new(0),
            generation_tx: gen_tx,
            reload_notify: Arc::new(Notify::new()),
            viewport_tx,
            load_in_flight: Mutex::new(None),
        });

        // 1. total_provider runs once per open, result cached.
        if let Some(cb) = dio.lens.callbacks.total_provider.as_ref() {
            let dio_handle = Dio { inner: dio.clone() };
            let total = cb(&dio_handle).await?;
            *state.total.write().unwrap() = Some(total);
        }

        // 2. Seed the sparse map from whatever's in the cache.
        state.reseed_from_cache().await?;
        state.bump_generation();

        // 3. Spawn reactor.
        let bus_rx = dio.event_bus.subscribe();
        let reactor_state = state.clone();
        dio.lens.runtime.spawn(async move {
            reload_loop(reactor_state, bus_rx).await;
        });

        // 4. Spawn viewport-debounce loop.
        let viewport_state = state.clone();
        let debounce = dio.lens.defaults.viewport_debounce;
        dio.lens.runtime.spawn(async move {
            viewport_loop(viewport_state, viewport_rx, debounce).await;
        });

        // 5. Optional refresh-on-open: schedule a viewport for the
        //    initial range so the configured on_load_chunk re-fetches
        //    the first page in the background.
        if dio.lens.defaults.refresh_on_open && dio.lens.callbacks.on_load_chunk.is_some() {
            let range = initial_range.unwrap_or(0..page_size);
            enqueue_viewport(
                &state,
                ViewportRequest {
                    range,
                    force_load: false,
                },
            );
        }

        Ok(Arc::new(TableSceneryImpl { inner: state }) as Arc<dyn TableScenery>)
    }
}
