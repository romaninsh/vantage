//! Building [`SharedRows`]: empty for an open, or as a copy of the rows a
//! view leaves when its query changes.
//!
//! The copy is what keeps a re-pointed view on screen: its new rows start as
//! the old ones and the new query's load replaces them in place, the way a
//! paged refetch overwrites slots as they land. The old rows are never
//! touched, so every other view of the old query keeps reading them.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use tokio::sync::{Notify, mpsc};
use vantage_vista::{Condition, VistaQuery};

use crate::dio::DioInner;
use crate::lens::cache_backend::CacheTable;
use crate::scenery::table::ViewportRequest;
use crate::scenery::table::loader::viewport_loop;
use crate::scenery::table::plan::Serving;
use crate::scenery::table::reactor::reload_loop;
use crate::scenery::table::two_pass::{reseed_loop, variant_key};
use crate::scenery::table::view::{ViewHook, ViewId};
use crate::scenery::table::view_stats::{StatsCell, TotalKind};

use super::{SharedRows, TaskGuard};

/// What a view hands over to build its rows.
pub(crate) struct Init {
    pub(crate) key: String,
    pub(crate) query: VistaQuery,
    pub(crate) base: Vec<Condition>,
    pub(crate) page_size: usize,
    pub(crate) titles_only: bool,
    pub(crate) paged: bool,
    pub(crate) read_cache: Option<Arc<dyn CacheTable>>,
    pub(crate) sugar_columns: Arc<HashSet<String>>,
}

impl SharedRows {
    /// Empty rows for `init` with `view` attached, the query planned. The
    /// tasks wait for [`start`](Self::start), so the caller can seed first.
    pub(crate) fn new(
        dio: &Arc<DioInner>,
        init: Init,
        view: (ViewId, ViewHook),
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<ViewportRequest>) {
        let (rows, rx, _) = Self::build(dio, init, view, None);
        (rows, rx)
    }

    /// Rows for `init` that start as a copy of `old`'s — the rows, their
    /// places, the total and whether they had settled — with `view` attached
    /// and the tasks running. The caller reloads them for the new query
    /// unless the returned flag says the change of serving already did.
    pub(crate) fn copied_from(
        old: &SharedRows,
        dio: &Arc<DioInner>,
        init: Init,
        view: (ViewId, ViewHook),
    ) -> (Arc<Self>, bool) {
        let (rows, rx, reloaded) = Self::build(dio, init, view, Some(old));
        rows.start(dio, rx);
        (rows, reloaded)
    }

    fn build(
        dio: &Arc<DioInner>,
        init: Init,
        view: (ViewId, ViewHook),
        copy: Option<&SharedRows>,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<ViewportRequest>, bool) {
        let (viewport_tx, viewport_rx) = mpsc::unbounded_channel();
        let two_pass = dio.has_dio_augment();
        let sweep_ticket = two_pass.then(|| {
            dio.ensure_augment_workers();
            Arc::new(dio.augment_scheduler.ticket())
        });
        let rows = Arc::new(SharedRows {
            dio_weak: Arc::downgrade(dio),
            key: init.key,
            read_cache: init.read_cache,
            sugar_columns: init.sugar_columns,
            query: init.query,
            base: init.base,
            rows: RwLock::new(Default::default()),
            id_to_idx: RwLock::new(HashMap::new()),
            total: RwLock::new(None),
            total_kind: RwLock::new(TotalKind::Exact),
            stats: StatsCell::new(),
            views: Mutex::new(HashMap::from([view])),
            page_size: init.page_size,
            reload_notify: Arc::new(Notify::new()),
            viewport_tx,
            viewport_queue_depth: AtomicUsize::new(0),
            load_in_flight: Mutex::new(None),
            load_dirty: AtomicBool::new(false),
            load_push_count: AtomicUsize::new(0),
            load_total_reported: AtomicBool::new(false),
            total_ever_stated: AtomicBool::new(false),
            settled: AtomicBool::new(false),
            placed: AtomicBool::new(false),
            omits_failed_rows: AtomicBool::new(false),
            paged: init.paged,
            master_capabilities: *dio.master.capabilities(),
            two_pass,
            view_plan: RwLock::new(None),
            serving: RwLock::new(copy.map_or(Serving::Windows, SharedRows::serving)),
            titles_only: init.titles_only,
            index: RwLock::new(None),
            sweep_ticket,
            list_in_flight: Mutex::new(false),
            coalescer: Arc::default(),
            reseed_lock: tokio::sync::Mutex::new(()),
            debug_tap: dio.tap().clone(),
            last_debug_state: Mutex::new(None),
            payload_named: AtomicBool::new(false),
            last_served: Mutex::new(None),
            tasks: TaskGuard::default(),
        });
        // A copy keeps the old index until its reload re-points it: the rows
        // it copied are placed by that index.
        let index = match copy {
            Some(old) => old.index(),
            None => two_pass.then(|| dio.query_index(&variant_key(&rows, dio))),
        };
        rows.set_index(index);
        if let Some(old) = copy {
            rows.copy_rows(old);
        }
        let reloaded = rows.replan();
        rows.publish_view_stats();
        (rows, viewport_rx, reloaded)
    }

    /// Spawn the reactor, the viewport loop and, on two-pass rows, the
    /// coalesced reseed. Each holds the rows weakly.
    pub(crate) fn start(
        self: &Arc<Self>,
        dio: &Arc<DioInner>,
        viewport_rx: mpsc::UnboundedReceiver<ViewportRequest>,
    ) {
        let bus = dio.event_bus.subscribe();
        let reactor = reload_loop(Arc::downgrade(self), self.reload_notify.clone(), bus);
        self.tasks.hold(dio.lens.runtime.spawn(reactor));
        let debounce = dio.lens.tuning().viewport_debounce;
        if self.two_pass {
            let reseeds = reseed_loop(Arc::downgrade(self), self.coalescer.clone(), debounce);
            self.tasks.hold(dio.lens.runtime.spawn(reseeds));
        }
        let viewport = viewport_loop(Arc::downgrade(self), viewport_rx, debounce);
        self.tasks.hold(dio.lens.runtime.spawn(viewport));
    }

    fn copy_rows(&self, old: &SharedRows) {
        *self.rows.write().unwrap() = old.rows.read().unwrap().clone();
        *self.id_to_idx.write().unwrap() = old.id_to_idx.read().unwrap().clone();
        *self.total.write().unwrap() = *old.total.read().unwrap();
        *self.total_kind.write().unwrap() = *old.total_kind.read().unwrap();
        for (to, from) in [
            (&self.settled, &old.settled),
            (&self.total_ever_stated, &old.total_ever_stated),
        ] {
            to.store(from.load(Ordering::SeqCst), Ordering::SeqCst);
        }
    }
}
