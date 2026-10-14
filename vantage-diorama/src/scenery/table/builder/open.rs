//! Opening a view: find or build the rows that answer its query, then attach
//! a view its caller owns.

use std::sync::{Arc, Mutex, RwLock};

use vantage_core::{Result, error};
use vantage_vista::VistaCapabilities;

use crate::dio::{DioInner, shared_key};
use crate::scenery::sugar::SugaredCache;
use crate::scenery::table::cap::Cap;
use crate::scenery::table::handle::TableSceneryImpl;
use crate::scenery::table::loader::enqueue_viewport;
use crate::scenery::table::state::{Init, SharedRows};
use crate::scenery::table::view::{TableView, ViewHook, ViewId, ViewSignal, compose, follow_tail};
use crate::scenery::table::{TableScenery, ViewportRequest};

use super::TableSceneryBuilder;
use super::seed::{cached_count, seed};

impl TableSceneryBuilder {
    /// Open a view. It reads the live rows of another view of the same query
    /// when there are some — no seeding, no fetch — and otherwise builds them:
    /// seeds the sparse map from the cache, spawns the reactor and the
    /// viewport loop, and schedules the on-open fetch. Requests a background
    /// refresh when the cache is older than the refresher's `max_age`.
    ///
    /// Every open returns a view of its own: its setters reshape it alone.
    ///
    /// Fails when the builder asked for both [`limit`](Self::limit) and
    /// [`tail`](Self::tail), or for a tail with no order to take it from.
    pub async fn open(self) -> Result<Arc<dyn TableScenery>> {
        let TableSceneryBuilder {
            dio,
            filters,
            order,
            search,
            limit,
            tail,
            page_size,
            titles_only,
            demand,
            sugar,
        } = self;
        // The master already holds every clause it accepted; the Dio's
        // residual reaches the view through its plan. Without an order of its
        // own the view keeps the Dio's, so a cache filled in arbitrary order
        // still reads back sorted.
        let order = order.or_else(|| dio.query.order.clone());
        let cap = match (limit, tail) {
            (Some(_), Some(_)) => {
                return Err(error!(
                    "limit and tail are exclusive",
                    table = dio.master.name()
                ));
            }
            (Some(n), None) => Some(Cap::Limit(n)),
            (None, Some(_)) if order.is_none() => {
                return Err(error!("a tail needs an order", table = dio.master.name()));
            }
            (None, Some(n)) => Some(Cap::Tail(n)),
            (None, None) => None,
        };
        dio.refresh_if_stale();
        let sugared = sugar.is_some();
        let sugar_columns = sugar.as_ref().map(|s| s.outputs()).unwrap_or_default();
        let read_cache = sugar.map(|s| SugaredCache::wrap(dio.cache.clone(), s));

        let load_order = cap.map_or(order.clone(), |cap| cap.load_order(order.clone()));
        let query = compose(&filters, &[], search.clone(), load_order);
        let key = shared_key(&dio, &query, titles_only);

        let id = ViewId::next();
        let signal = Arc::new(ViewSignal::new());
        // Two-pass hydration runs on the Dio's central augment scheduler:
        // make sure its workers exist and give this view its own place in it.
        let ticket = dio.has_dio_augment().then(|| {
            dio.ensure_augment_workers();
            Arc::new(dio.augment_scheduler.ticket())
        });
        let hook = || ViewHook::new(&signal, ticket.clone());

        // A sugared view never shares: other views don't read its outputs.
        // Rows whose last view left in between refuse the attach, and the
        // view builds its own.
        let live = if sugared {
            None
        } else {
            dio.shared_rows(&key)
                .filter(|rows| rows.try_attach(id, hook()).is_ok())
        };
        let shared = match live {
            Some(rows) => rows,
            None => {
                let init = Init {
                    key: key.clone(),
                    query,
                    base: filters.clone(),
                    page_size,
                    titles_only,
                    paged: pages_lazily(&dio, dio.master.capabilities()),
                    read_cache,
                    sugar_columns,
                };
                let (rows, viewport_rx) = SharedRows::new(&dio, init, (id, hook()));
                seed(&rows, &dio).await?;
                rows.start(&dio, viewport_rx);
                fetch_on_open(&rows, &dio, id).await;
                if sugared {
                    rows
                } else {
                    register(&dio, key, rows, id, &hook)
                }
            }
        };

        let view = Arc::new(TableView {
            _tally: crate::stats::Tally::table_scenery(),
            id,
            dio_weak: Arc::downgrade(&dio),
            base: filters,
            filters: RwLock::new(Vec::new()),
            search: RwLock::new(search),
            order: RwLock::new(order),
            cap: RwLock::new(cap),
            placed_end: Mutex::new(None),
            refused: RwLock::new(None),
            shared: RwLock::new(shared),
            shown: RwLock::new(None),
            page_size,
            titles_only,
            demand,
            sugared,
            ticket,
            signal,
            master_capabilities: *dio.master.capabilities(),
            repointing: Mutex::new(()),
        });
        dio.register_view(&view);
        if let Some(Cap::Tail(n)) = cap {
            // The whole tail is the view: ask for it before any consumer
            // does, and keep it on the end of the set as that end moves.
            view.set_viewport(0..n, 0..0);
            let follow = follow_tail(
                Arc::downgrade(&view),
                view.signal.subscribe(),
                view.signal.subscribe_stats(),
            );
            dio.lens.runtime.spawn(follow);
        }
        dio.emit_census("table scenery", "opened");
        Ok(Arc::new(TableSceneryImpl { view }))
    }
}

/// Register freshly built `rows` under `key`. If a concurrent open won the
/// race for this key, join its rows; ours drop here and their tasks abort.
/// Should the winner's last view leave before the join, registering again
/// replaces it with ours.
fn register(
    dio: &DioInner,
    key: String,
    rows: Arc<SharedRows>,
    id: ViewId,
    hook: &dyn Fn() -> ViewHook,
) -> Arc<SharedRows> {
    loop {
        let winner = dio.register_shared_rows(key.clone(), rows.clone());
        if Arc::ptr_eq(&winner, &rows) || winner.try_attach(id, hook()).is_ok() {
            return winner;
        }
    }
}

/// Optional refresh-on-open: schedule the first page so the paged loader
/// re-fetches it in the background, as the opening view's request. Skipped
/// for two-pass — its detail pass waits for an explicit viewport, so opening
/// triggers no detail fetches. The one exception is a view whose predicate
/// names an augmented value: it sweeps its whole index as soon as the list
/// pass has listed it, viewport or not, because it can't settle until every
/// row is hydrated (see `two_pass::detail::resume_hydration`).
///
/// `force_load` so the fetch runs even when the cache seed already filled
/// this range: the cache is id-keyed (arbitrary order), but the server
/// applies the query's ordering, so the on-open fetch must actually hit the
/// server to replace the seed with ordered rows — otherwise the grid shows
/// cache order until a manual refresh. It is the opening view's request, so
/// the grid's first real viewport, pushed inside the debounce window,
/// coalesces with it and keeps the force.
async fn fetch_on_open(rows: &SharedRows, dio: &Arc<DioInner>, view: ViewId) {
    if !dio.lens.tuning().refresh_on_open || !rows.is_chunk_loaded() {
        return;
    }
    let range = 0..rows.page_size;
    // `force_load` means this fetch runs even when the cache already covers
    // the range — i.e. opening ALWAYS goes to the master. That is the other
    // half of why a warm cache saves nothing here. Read before the macro: the
    // count is awaited, and a lock taken inside the argument list would be
    // held across it.
    let table = dio.master.name().to_string();
    let cached_rows = cached_count(dio).await;
    tracing::debug!(
        target: "vantage_diorama::cache",
        table = %table,
        ?range,
        cached_rows,
        "on-open fetch scheduled (force_load) — the master is consulted \
         regardless of what the cache holds",
    );
    enqueue_viewport(
        rows,
        ViewportRequest {
            range,
            force_load: true,
            // An empty cache means the grid shows nothing until this
            // returns; a warm one means it is a re-pull behind rows already
            // on screen.
            priority: if cached_rows == 0 {
                vantage_core::Priority::Essential
            } else {
                vantage_core::Priority::Background
            },
            view: Some(view),
        },
    );
}

/// Whether this view pages rows in lazily rather than warming from the cache.
///
/// A loader with both halves (`Loader::paged(..).or_eager(..)`) decides by the
/// **master**: a source that can serve `[offset, limit)` is paged, and one that
/// cannot is warmed whole, so one Lens serves both shapes. A loader with one
/// half keeps it regardless of capability: a paged loader over a master that
/// doesn't advertise `Window` is legitimate — the loader may know how to window
/// a source the Vista can't describe.
fn pages_lazily(dio: &Arc<DioInner>, master: &VistaCapabilities) -> bool {
    dio.lens.callbacks.loader.pages(master)
}
