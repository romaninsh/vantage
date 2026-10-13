use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{Arc, Mutex, RwLock};

use ciborium::Value as CborValue;
use tokio::sync::{Notify, mpsc, watch};
use vantage_core::Result;
use vantage_vista::{Capability, VistaCapabilities};

use crate::dio::query_index::index_key;
use crate::dio::{DioInner, Generation};

use super::handle::{SceneryGuard, TableSceneryImpl};
use super::helpers::op_conditions_key;
use super::loader::{enqueue_viewport, viewport_loop};
use super::reactor::reload_loop;
use super::state::TableSceneryState;
use super::{SortDir, TableScenery, ViewportRequest};

/// Builder produced by [`Dio::table_scenery`](crate::Dio::table_scenery).
pub struct TableSceneryBuilder {
    pub(crate) dio: Arc<DioInner>,
    pub(crate) conditions: Vec<(String, CborValue)>,
    /// Non-equality filters applied locally over the cache (the fallback for
    /// operators the master vista can't push down). Kept separate from the
    /// equality `conditions` so the plain eq path is untouched.
    pub(crate) op_conditions: Vec<super::OpCondition>,
    pub(crate) sort: Option<(String, SortDir)>,
    pub(crate) page_size: usize,
    pub(crate) initial_range: Option<std::ops::Range<usize>>,
    pub(crate) titles_only: bool,
    pub(crate) demand: Option<Vec<String>>,
    pub(crate) exclusive: bool,
    pub(crate) sugar: Option<crate::scenery::sugar::Sugar>,
}

impl TableSceneryBuilder {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        Self {
            dio,
            conditions: Vec::new(),
            op_conditions: Vec::new(),
            sort: None,
            page_size: 100,
            initial_range: None,
            titles_only: false,
            demand: None,
            exclusive: false,
            sugar: None,
        }
    }

    /// Read rows through `sugar`: its outputs are merged into every row
    /// this view reads — sort and filter included — and nothing reaches the
    /// cache. A sugared view is always exclusive, so it never shares a
    /// plain view of the same query.
    pub fn sugar(mut self, sugar: crate::scenery::sugar::Sugar) -> Self {
        self.sugar = Some(sugar);
        self.exclusive = true;
        self
    }

    pub fn where_eq(mut self, col: impl Into<String>, value: impl Into<CborValue>) -> Self {
        self.conditions.push((col.into(), value.into()));
        self
    }

    /// Add a non-equality filter (`!=`, `<`, `in`, …) applied locally over the
    /// cache. Use for operators the master vista can't push down; a plain
    /// equality is cheaper through [`where_eq`](Self::where_eq).
    pub fn where_op(
        mut self,
        col: impl Into<String>,
        op: vantage_vista::FilterOp,
        value: impl Into<CborValue>,
    ) -> Self {
        self.op_conditions
            .push(super::OpCondition::new(col, op, value));
        self
    }

    pub fn sort(mut self, col: impl Into<String>, dir: SortDir) -> Self {
        self.sort = Some((col.into(), dir));
        self
    }

    /// Hint range used by `request_load_more` and by the
    /// refresh-on-open initial fetch. Default 100.
    pub fn page_size(mut self, n: usize) -> Self {
        self.page_size = n;
        self
    }

    /// Override the initial range fetched at open time when the
    /// lens's `refresh_on_open` is enabled. Default `0..page_size`.
    pub fn initial_range(mut self, range: std::ops::Range<usize>) -> Self {
        self.initial_range = Some(range);
        self
    }

    /// Project to the cheap title columns only — the dropdown / autocomplete
    /// shape. On an **augmented (two-pass)** table this suppresses the detail
    /// pass entirely: the scenery serves the list-pass rows (id + title columns)
    /// and never pays for per-row hydration, so a 10,000-row lookup opens a
    /// picker as cheaply as it lists. Same mechanic as a grid otherwise —
    /// `set_viewport` for the visible band. A
    /// `titles_only` picker and a full grid over the same query are distinct
    /// sceneries (the grid hydrates, the picker doesn't).
    pub fn titles_only(mut self) -> Self {
        self.titles_only = true;
        self
    }

    /// Declare which columns this view actually shows — its **demand**.
    /// Default (`None`) demands everything, the pre-demand behavior. Demand
    /// gates the AUGMENT detail pass only: the Dio unions the demands of its
    /// open sceneries and fetches augment values only while some open view
    /// demands an augmented column (a tree of folder names never pays for
    /// folder sizes; the listing beside it does). Non-augment (list-pass)
    /// columns always flow regardless of demand. Recomputed naturally as
    /// sceneries open and close; already-merged values stay when demand
    /// drains — they just stop refreshing.
    pub fn columns<I, S>(mut self, columns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.demand = Some(columns.into_iter().map(Into::into).collect());
        self
    }

    /// Opt out of dedup **sharing**: this view always gets its own scenery —
    /// its own viewport, its own hydration queue — instead of joining a live
    /// scenery for the same query. For per-consumer standing views (an HTTP
    /// watch connection, one grid per client) sharing is wrong: every
    /// consumer's `set_viewport` would fight over the one shared window, and
    /// only the last writer's rows would hydrate. An exclusive scenery still
    /// registers (under a unique key), so its demand joins the union and its
    /// close still drains it; per-query indexes stay shared either way.
    pub fn exclusive(mut self) -> Self {
        self.exclusive = true;
        self
    }

    /// Open the Scenery — seeds the sparse map from the cache, spawns the
    /// reactor and viewport-debounce tasks, optionally schedules a background
    /// initial-load, and requests a background refresh when the cache is
    /// older than the refresher's `max_age`. Returns the live handle.
    pub async fn open(self) -> Result<Arc<dyn TableScenery>> {
        self.dio.refresh_if_stale();
        let TableSceneryBuilder {
            dio,
            conditions,
            op_conditions,
            sort,
            page_size,
            initial_range,
            titles_only,
            demand,
            exclusive,
            sugar,
        } = self;
        let sugar_columns = sugar.as_ref().map(|s| s.outputs()).unwrap_or_default();
        let read_cache =
            sugar.map(|s| crate::scenery::sugar::SugaredCache::wrap(dio.cache.clone(), s));

        // The master already holds every clause it accepted; the Dio's
        // residual reaches the view through its plan. Without an order of its
        // own the view keeps the Dio's, so a cache filled in arbitrary order
        // still reads back sorted.
        let sort = sort.or_else(|| {
            dio.query.order.as_ref().map(|(column, dir)| {
                let dir = match dir {
                    vantage_vista::SortDirection::Ascending => SortDir::Asc,
                    vantage_vista::SortDirection::Descending => SortDir::Desc,
                };
                (column.clone(), dir)
            })
        });

        // Dedup key over (shape, conditions, sort, titles_only). A live
        // scenery for the same query is shared — one reactor, one cache window,
        // one in-flight JoinSet — instead of standing up a parallel copy. A
        // `titles_only` picker keys distinctly from a full grid so the picker
        // never inherits the grid's detail hydration.
        let key = {
            // Demand joins the dedup key: a tree demanding [name, kind] and a
            // grid demanding [.., size] over the same query are DISTINCT
            // sceneries — sharing one would erase the cheaper view's savings.
            let demand_key = match &demand {
                None => "*".to_string(),
                Some(columns) => {
                    let mut sorted = columns.clone();
                    sorted.sort();
                    sorted.join(",")
                }
            };
            let mut key = format!(
                "table\u{1}{}\u{1}{}\u{1}{}\u{1}{}",
                index_key(&dio.master, &conditions, sort.as_ref()),
                op_conditions_key(&op_conditions),
                titles_only as u8,
                demand_key,
            );
            // An exclusive view registers under a unique key: never shared,
            // but still visible to the demand union and the diagnostics.
            if exclusive {
                use std::sync::atomic::{AtomicU64, Ordering};
                static EXCLUSIVE_SEQ: AtomicU64 = AtomicU64::new(0);
                key.push('\u{1}');
                key.push_str(&EXCLUSIVE_SEQ.fetch_add(1, Ordering::Relaxed).to_string());
            }
            key
        };
        if let Some(existing) = dio.lookup_table_scenery(&key) {
            return Ok(existing);
        }

        let (gen_tx, _gen_rx) = watch::channel(Generation::default());
        let (viewport_tx, viewport_rx) = mpsc::unbounded_channel();

        let master_capabilities = *dio.master.capabilities();

        // Two-pass engages when the Dio owns augmentation. The shared per-query
        // index is keyed by the master Vista's index key over the scenery's
        // conditions + sort, so reopening the same variant reuses the
        // already-built index.
        let two_pass = dio.has_dio_augment();
        // Whether the visible set is refined locally is derived from the query
        // as it stands — see `TableSceneryState::local_refine`. It is not
        // captured here: `set_sort` changes the answer, and a
        // value frozen at open made a later sort a silent no-op.
        let index = if two_pass {
            let mut key = index_key(&dio.master, &conditions, sort.as_ref());
            // Operator filters live outside the eq-only index key, so
            // fold them into the shared-index key here — two op-filter variants
            // must not collide onto one ordered index.
            key.push('\u{1}');
            key.push_str(&op_conditions_key(&op_conditions));
            Some(dio.query_index(&key))
        } else {
            None
        };

        // Two-pass hydration runs on the Dio's central augment scheduler:
        // make sure its workers exist and register this view as a requester.
        let augment_ticket = if two_pass {
            dio.ensure_augment_workers();
            Some(dio.augment_scheduler.ticket())
        } else {
            None
        };

        let state = Arc::new(TableSceneryState {
            _tally: crate::stats::Tally::table_scenery(),
            dio_weak: Arc::downgrade(&dio),
            read_cache,
            sugar_columns,
            conditions: RwLock::new(conditions),
            op_conditions: RwLock::new(op_conditions),
            sort: RwLock::new(sort),
            rows: RwLock::new(Default::default()),
            id_to_idx: RwLock::new(HashMap::new()),
            total: RwLock::new(None),
            total_kind: RwLock::new(super::view_stats::TotalKind::Exact),
            stats: super::view_stats::StatsCell::new(),
            last_viewport: RwLock::new(None),
            page_size,
            generation: AtomicU64::new(0),
            generation_tx: gen_tx,
            reload_notify: Arc::new(Notify::new()),
            viewport_tx,
            viewport_queue_depth: AtomicUsize::new(0),
            load_in_flight: Mutex::new(None),
            load_dirty: std::sync::atomic::AtomicBool::new(false),
            load_push_count: AtomicUsize::new(0),
            load_total_reported: std::sync::atomic::AtomicBool::new(false),
            total_ever_stated: std::sync::atomic::AtomicBool::new(false),
            paged: pages_lazily(&dio, &master_capabilities),
            settled: std::sync::atomic::AtomicBool::new(false),
            omits_failed_rows: std::sync::atomic::AtomicBool::new(false),
            master_capabilities,
            two_pass,
            ui_filters: RwLock::new(Vec::new()),
            ui_terms: RwLock::new(Vec::new()),
            search: RwLock::new(None),
            view_plan: RwLock::new(None),
            serving: RwLock::new(super::plan::Serving::Windows),
            titles_only,
            demand,
            index: RwLock::new(index),
            registry_key: Mutex::new(Some(key.clone())),
            augment_ticket,
            list_in_flight: Mutex::new(false),
            debug_tap: dio.tap().clone(),
            last_debug_state: Mutex::new(None),
            payload_named: std::sync::atomic::AtomicBool::new(false),
            last_served: Mutex::new(None),
        });
        state.replan();

        // What the source can do decides everything that follows — which work
        // is pushed down, and which the layers above have to do themselves.
        // Said once, at open, so the rest of the session reads against it.
        {
            let caps = &state.master_capabilities;
            // Every shell counts, so `count` is never missing.
            let can: Vec<&str> = [
                Some("count"),
                caps.has(Capability::Window).then_some("window"),
                caps.has(Capability::Cursor).then_some("cursor"),
                caps.has(Capability::Order).then_some("order"),
                caps.has(Capability::Search).then_some("search"),
            ]
            .into_iter()
            .flatten()
            .collect();
            let cannot: Vec<&str> = [
                (!caps.has(Capability::Order)).then_some("order"),
                (!caps.has(Capability::Search)).then_some("search"),
            ]
            .into_iter()
            .flatten()
            .collect();
            crate::debug::tapline!(
                state.debug_tap,
                "source",
                "{}{} · {}",
                if can.is_empty() {
                    "can do nothing but list".to_string()
                } else {
                    format!("can {}", can.join(", "))
                },
                if cannot.is_empty() {
                    String::new()
                } else {
                    format!(" · cannot {}", cannot.join(", "))
                },
                if state.paged {
                    format!("pages lazily, {} rows at a time", state.page_size)
                } else {
                    "copied whole into the cache".to_string()
                },
            );
        }

        // 1. Seed the sparse map.
        if state.two_pass {
            // Two-pass: seed from the shared per-query index if it is already
            // populated (reused variant — no list call); otherwise run the
            // first list page. The detail pass stays dormant until a viewport
            // is set, so opening yields `Incomplete` rows with zero detail
            // calls.
            // A refined view evaluates its query over the whole index, so a
            // shared index still being listed is one it lists to the end.
            let index_ready = state
                .index()
                .is_some_and(|i| !i.is_empty() && (i.is_complete() || !state.local_refine()));
            if !index_ready {
                // The first list page is a query against the datasource, and
                // awaiting it here put a whole round trip on the open path —
                // which is a UI thread waiting to mount. On a slow source that
                // is the entire cost of opening the page: one measured
                // SurrealDB list of 872 rows took 13.5s, and the window was
                // frozen for all of it.
                //
                // So run it detached. The scenery opens in
                // `LoadState::Loading`, the rows arrive with a generation bump,
                // and consumers that need rows wait for the state to leave
                // `Loading` rather than assuming `open()` implies data.
                let seed_state = state.clone();
                dio.lens.runtime.spawn(async move {
                    super::two_pass::run_list_page(seed_state.clone()).await;
                    // A viewport set before the list landed found an empty
                    // index and enqueued nothing, so hydration would wait for
                    // the user to scroll. Re-drive it now that there are rows
                    // to hydrate — the same restart the reorder path does.
                    seed_state.refresh_loaded_viewport(vantage_core::Priority::Background);
                });
            } else {
                // A populated shared index costs no query — it is already in
                // memory, so seeding from it stays inline and the scenery opens
                // with its rows. A refined view publishes only its refined
                // map, never the index's raw rows.
                // A refined view settles itself once its map answers the
                // query, which on an augmented value means once its sweep is
                // done; the sweep starts here.
                if state.local_refine() {
                    super::two_pass::reseed_filtered(&state).await;
                    super::two_pass::resume_hydration(&state);
                } else {
                    super::two_pass::seed_from_index(&state).await;
                    state.mark_settled("open: seeded from shared index");
                }
                state.bump_generation();
            }
        } else if state.paged && !state.is_chunk_loaded() {
            // A paged view whose query the master can't answer whole. Either
            // it failed already (`replan` withheld the rows), or the reactor
            // copies the whole set and refines it; the view opens loading
            // until then.
            if !matches!(state.serving(), super::plan::Serving::Failed(_)) {
                state.reload_notify.notify_one();
            }
            state.bump_generation();
        } else if state.paged {
            // A paged view: rows arrive a window at a time, and no eager start
            // ran for this Dio.
            //
            // A WARM cache is shown at once rather than thrown away: navigating
            // back to a grid whose rows are still cached must not flash a
            // loading skeleton over data we already hold. `reseed_from_cache`
            // orders the cached rows by the view's active sort, so a sorted
            // grid opens in the right order; the `refresh_on_open` force-fetch
            // below then replaces the visible window with the authoritative
            // server rows *silently* — the view is already settled, so the swap
            // carries no loading feedback. If the client sort can't reproduce
            // the server's order exactly, the first screen reshuffles once when
            // the fetch lands; the alternative is a skeleton over cached rows,
            // which is worse. A non-contiguous cache compacts here, but the
            // forced refetch corrects the first screen and scrolling corrects
            // the rest.
            //
            // A COLD cache leaves the map empty so the grid shows its loading
            // state; the on-open fetch is authoritative and fills the master's
            // order. This is the "cache is not present" case the loading
            // skeleton exists for.
            //
            // Both reads happen before the macro: the count is awaited, and a
            // lock taken inside the argument list would be held across it.
            let table = dio.master.name().to_string();
            let cached_rows = cached_count(&dio).await;
            if cached_rows > 0 {
                state.reseed_from_cache().await?;
                // The remembered total is the whole table's, so a view whose
                // query narrows the set doesn't take it.
                if !state.view_narrows() {
                    restore_meta_total(&state, &dio).await;
                }
                state.mark_settled("open: paged view seeded from warm cache");
                tracing::debug!(
                    target: "vantage_diorama::cache",
                    table = %table,
                    cached_rows,
                    "warm cache seeded the paged view; refresh-on-open replaces it silently",
                );
                crate::debug::tapline!(
                    state.debug_tap,
                    "scenery",
                    "opened warm — {} rows already on disk",
                    crate::debug::num(cached_rows.max(0) as usize),
                );
            } else {
                tracing::debug!(
                    target: "vantage_diorama::cache",
                    table = %table,
                    cached_rows,
                    "cache empty — paged view opens loading; the on-open fetch is authoritative",
                );
                crate::debug::tapline!(
                    state.debug_tap,
                    "scenery",
                    "opened cold — nothing cached on disk yet",
                );
            }
            state.bump_generation();
        } else {
            // Eager single-pass: the cache IS the row set; seed (and order)
            // from it directly.
            state.reseed_from_cache().await?;
            let seeded_rows = state.rows.read().unwrap().len();
            tracing::debug!(
                target: "vantage_diorama::cache",
                table = %dio.master.name(),
                seeded_rows,
                "cache seeded the visible map",
            );
            crate::debug::tapline!(
                state.debug_tap,
                "scenery",
                "opened {} — {} rows seeded from cache",
                if seeded_rows > 0 { "warm" } else { "cold" },
                crate::debug::num(seeded_rows),
            );
            // The cache IS the row set for this shape, so a seed that found
            // rows IS the answer. A seed that found none is not: a cold cache
            // with a background copy still in flight looks exactly like an
            // empty table, and calling it settled here is what made the grid
            // skip its loading state and paint "no rows" over a load that had
            // not started. Wait for the reactor's reseed in that case.
            //
            // Unless nothing more is coming — no eager start is still running
            // (`seed_complete`), so zero rows is a real answer and there will
            // be no second reseed to wait for. The reactor applies the same
            // rule.
            let seeded_any = !state.rows.read().unwrap().is_empty();
            if seeded_any || dio.seed_complete.load(std::sync::atomic::Ordering::SeqCst) {
                state.mark_settled("open: seeded from cache");
            }
            state.bump_generation();
        }

        // 2. Spawn reactor. Handle retained by the scenery's drop guard so a
        //    released scenery stops reacting (and frees its state) instead of
        //    living for the Dio's whole lifetime.
        let bus_rx = dio.event_bus.subscribe();
        let reactor_state = state.clone();
        let reactor_handle = dio.lens.runtime.spawn(async move {
            reload_loop(reactor_state, bus_rx).await;
        });

        // 3. Spawn viewport-debounce loop. This task owns every in-flight
        //    fetch inline, so aborting it on drop cancels outstanding loads —
        //    a closing grid stops pulling.
        let viewport_state = state.clone();
        let debounce = dio.lens.tuning().viewport_debounce;
        let viewport_handle = dio.lens.runtime.spawn(async move {
            viewport_loop(viewport_state, viewport_rx, debounce).await;
        });

        // 4. Optional refresh-on-open: schedule a viewport for the
        //    initial range so the paged loader re-fetches the first page
        //    in the background. Skipped for two-pass — its detail pass waits
        //    for an explicit viewport, so opening triggers no detail fetches.
        //    The one exception is a view whose predicate names an augmented
        //    value: it sweeps its whole index as soon as the list pass has
        //    listed it, viewport or not, because it can't settle until every
        //    row is hydrated (see `two_pass::detail::resume_hydration`).
        //
        //    `force_load` so the fetch runs even when the cache seed already
        //    filled this range: the cache is id-keyed (arbitrary order), but the
        //    server applies the query's ordering, so the on-open fetch must
        //    actually hit the server to replace the seed with ordered rows —
        //    otherwise the grid shows cache order until a manual refresh.
        if dio.lens.tuning().refresh_on_open && state.is_chunk_loaded() {
            let range = initial_range.unwrap_or(0..page_size);
            // `force_load` means this fetch runs even when the cache already
            // covers the range — i.e. opening ALWAYS goes to the master. That
            // is the other half of why a warm cache saves nothing here.
            // Read before the macro — see the sibling log above.
            let table = dio.master.name().to_string();
            let cached_rows = cached_count(&dio).await;
            tracing::debug!(
                target: "vantage_diorama::cache",
                table = %table,
                ?range,
                cached_rows,
                "on-open fetch scheduled (force_load) — the master is consulted \
                 regardless of what the cache holds",
            );
            enqueue_viewport(
                &state,
                ViewportRequest {
                    range,
                    force_load: true,
                    // An empty cache means the grid shows nothing until this
                    // returns; a warm one means it is a re-pull behind rows
                    // already on screen.
                    priority: if cached_rows == 0 {
                        vantage_core::Priority::Essential
                    } else {
                        vantage_core::Priority::Background
                    },
                },
            );
        }

        let scenery: Arc<dyn TableScenery> = Arc::new(TableSceneryImpl {
            _guard: SceneryGuard {
                tasks: vec![reactor_handle, viewport_handle],
                dio_weak: state.dio_weak.clone(),
            },
            inner: state,
        });

        // Publish to the dedup registry. If a concurrent open won the race for
        // this key, `register_table_scenery` returns the winner and our
        // `scenery` drops here — its guard aborts the redundant tasks.
        let scenery = dio.register_table_scenery(key, scenery);
        dio.emit_census("table scenery", "opened");
        Ok(scenery)
    }
}

/// Restore the grand total remembered by the last session's stated fetch
/// (see `CacheTable::meta_total`), so a warm reopen advertises its final row
/// count before any fetch runs. Latched as stated — it WAS stated, last time
/// — so the unknown-total horizon rule stays out of the way; the on-open
/// refetch re-states (and re-persists) the current truth. Until then the
/// view reports it as an estimate.
async fn restore_meta_total(state: &Arc<TableSceneryState>, dio: &Arc<DioInner>) {
    match dio.cache.meta_total().await {
        Ok(Some(total)) => {
            state
                .total_ever_stated
                .store(true, std::sync::atomic::Ordering::SeqCst);
            state.set_total_as(
                Some(total as usize),
                super::view_stats::TotalKind::Remembered,
            );
            tracing::debug!(
                target: "vantage_diorama::cache",
                table = %dio.master.name(),
                total,
                "restored the remembered grand total",
            );
        }
        Ok(None) => {}
        Err(e) => tracing::debug!(
            target: "vantage_diorama::cache",
            error = %e,
            "reading the remembered total failed — geometry arrives with the first fetch",
        ),
    }
}

/// How many rows the cache holds. A cache that can't count is opened as
/// cold, the safe reading: the view shows its loading state and the on-open
/// fetch fills it from the master. The failure is logged.
async fn cached_count(dio: &Arc<DioInner>) -> i64 {
    match dio.cache.count().await {
        Ok(count) => count,
        Err(e) => {
            tracing::warn!(
                target: "vantage_diorama::cache",
                table = %dio.cache_table_name,
                error = %e,
                "the cache can't count its rows; the view opens as over a cold cache",
            );
            0
        }
    }
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
