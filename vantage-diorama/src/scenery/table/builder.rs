use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{Arc, Mutex, RwLock};

use ciborium::Value as CborValue;
use tokio::sync::{Notify, mpsc, watch};
use vantage_core::Result;

use crate::dio::{Dio, DioInner, Generation};

use super::loader::{enqueue_viewport, viewport_loop};
use super::reactor::reload_loop;
use super::state::TableSceneryState;
use super::{SceneryGuard, SortDir, TableScenery, TableSceneryImpl, ViewportRequest};

/// Builder produced by [`Dio::table_scenery`](crate::Dio::table_scenery).
pub struct TableSceneryBuilder {
    pub(crate) dio: Arc<DioInner>,
    pub(crate) conditions: Vec<(String, CborValue)>,
    pub(crate) sort: Option<(String, SortDir)>,
    pub(crate) search: Option<String>,
    pub(crate) page_size: usize,
    pub(crate) initial_range: Option<std::ops::Range<usize>>,
    pub(crate) titles_only: bool,
    pub(crate) demand: Option<Vec<String>>,
    pub(crate) exclusive: bool,
}

impl TableSceneryBuilder {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        Self {
            dio,
            conditions: Vec::new(),
            sort: None,
            search: None,
            page_size: 100,
            initial_range: None,
            titles_only: false,
            demand: None,
            exclusive: false,
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
    /// `set_search` for typeahead, `set_viewport` for the visible band. A
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
            initial_range,
            titles_only,
            demand,
            exclusive,
        } = self;

        // Inherit the Dio's base query semantics. The Dio owns "what this table
        // is" (base conditions + default order); this view layers its own
        // conditions on top and falls back to the Dio's order when it sets none.
        let conditions = {
            let base = dio.base_conditions.read().unwrap();
            if base.is_empty() {
                conditions
            } else {
                let mut merged = base.clone();
                merged.extend(conditions);
                merged
            }
        };
        let sort = sort.or_else(|| dio.base_sort.read().unwrap().clone());

        // Dedup key over (shape, conditions, sort, search, titles_only). A live
        // scenery for the same query is shared — one reactor, one cache window,
        // one in-flight JoinSet — instead of standing up a parallel copy. A
        // `titles_only` picker keys distinctly from a full grid so the picker
        // never inherits the grid's detail hydration.
        let key = {
            let vista_sort = sort.as_ref().map(|(col, dir)| {
                let dir = match dir {
                    SortDir::Asc => vantage_vista::SortDirection::Ascending,
                    SortDir::Desc => vantage_vista::SortDirection::Descending,
                };
                (col.as_str(), dir)
            });
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
                dio.master
                    .read()
                    .unwrap()
                    .index_key(&conditions, vista_sort),
                search.as_deref().unwrap_or(""),
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

        let master_capabilities = dio.master.read().unwrap().capabilities().clone();

        // Two-pass engages when the Dio owns augmentation, or the Lens registers
        // an explicit `on_load_detail` callback. The shared per-query index is
        // keyed by the master Vista's index_key over the scenery's conditions +
        // sort, so reopening the same variant reuses the already-built index.
        let two_pass = dio.is_two_pass();
        // Two-pass can't push conditions/sort to its list pass, so any
        // condition/sort means the visible set must be refined locally over the
        // cache (this is also the only way an augmented-column filter can work —
        // the column exists only after hydration). A picker (`titles_only`) keeps
        // raw list order — it never hydrates, so it can't evaluate augmented
        // predicates anyway.
        let local_refine = two_pass
            && !titles_only
            && (!conditions.is_empty() || sort.is_some() || search.is_some());
        let index = if two_pass {
            let vista_sort = sort.as_ref().map(|(col, dir)| {
                let dir = match dir {
                    SortDir::Asc => vantage_vista::SortDirection::Ascending,
                    SortDir::Desc => vantage_vista::SortDirection::Descending,
                };
                (col.as_str(), dir)
            });
            let key = dio
                .master
                .read()
                .unwrap()
                .index_key(&conditions, vista_sort);
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
            dio_weak: Arc::downgrade(&dio),
            conditions: RwLock::new(conditions),
            sort: RwLock::new(sort),
            search: RwLock::new(search),
            rows: RwLock::new(Default::default()),
            id_to_idx: RwLock::new(HashMap::new()),
            total: RwLock::new(None),
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
            master_capabilities,
            two_pass,
            local_refine,
            titles_only,
            demand,
            index: RwLock::new(index),
            registry_key: Mutex::new(Some(key.clone())),
            augment_ticket,
            list_in_flight: Mutex::new(false),
        });

        // 1. total_provider runs once per open, result cached.
        if let Some(cb) = dio.lens.callbacks.total_provider.as_ref() {
            let dio_handle = Dio { inner: dio.clone() };
            let total = cb(&dio_handle).await?;
            *state.total.write().unwrap() = Some(total);
        }

        // 2. Seed the sparse map.
        if state.two_pass {
            // Two-pass: seed from the shared per-query index if it is already
            // populated (reused variant — no list call); otherwise run the
            // first list page. The detail pass stays dormant until a viewport
            // is set, so opening yields `Incomplete` rows with zero detail
            // calls.
            let index_empty = state.index().map(|i| i.is_empty()).unwrap_or(true);
            if index_empty {
                super::two_pass::run_list_page(state.clone()).await;
            } else {
                super::two_pass::seed_from_index(&state).await;
                state.bump_generation();
            }
            // Locally-refined views filter/sort the just-seeded rows over the
            // cache. With an augmented-column condition this yields an empty set
            // until hydration confirms matches.
            if state.local_refine {
                super::two_pass::reseed_filtered(&state).await;
                state.bump_generation();
            }
        } else if dio.lens.callbacks.on_load_chunk.is_some()
            && dio.lens.callbacks.on_start.is_none()
        {
            // Pure paged lens (lazy chunk loading, no eager `on_start` warm): the
            // row ORDER is the master's, fetched a page at a time — NOT the cache's
            // id-keyed iteration order. Seeding densely from the cache here would
            // both show the wrong order and make the viewport loader treat every
            // row as already-cached and skip the authoritative fetch, so a warm
            // cache (the redb file surviving a restart) would pin the grid to a
            // stale, mis-ordered set until a forced refresh. Leave the map empty;
            // the first viewport load fills it in the master's order.
            // `total_provider` (above) already sized the scrollbar, so the grid
            // shows its loading state, not a blank count.
            //
            // A hybrid lens that DOES warm via `on_start` (cache seeded in the
            // master's list order) still reseeds below, so its cache-aware
            // "skip already-loaded ranges" optimisation is preserved.
            state.bump_generation();
        } else {
            // Eager single-pass (or `on_start`-warmed hybrid): the cache IS the
            // row set; seed (and order) from it directly.
            state.reseed_from_cache().await?;
            state.bump_generation();
        }

        // 3. Spawn reactor. Handle retained by the scenery's drop guard so a
        //    released scenery stops reacting (and frees its state) instead of
        //    living for the Dio's whole lifetime.
        let bus_rx = dio.event_bus.subscribe();
        let reactor_state = state.clone();
        let reactor_handle = dio.lens.runtime.spawn(async move {
            reload_loop(reactor_state, bus_rx).await;
        });

        // 4. Spawn viewport-debounce loop. This task owns every in-flight
        //    fetch inline, so aborting it on drop cancels outstanding loads —
        //    a closing grid stops pulling.
        let viewport_state = state.clone();
        let debounce = dio.lens.defaults.viewport_debounce;
        let viewport_handle = dio.lens.runtime.spawn(async move {
            viewport_loop(viewport_state, viewport_rx, debounce).await;
        });

        // 5. Optional refresh-on-open: schedule a viewport for the
        //    initial range so the configured on_load_chunk re-fetches
        //    the first page in the background. Skipped for two-pass — its
        //    detail pass must wait for an explicit viewport so that opening
        //    never triggers detail fetches.
        //
        //    `force_load` so the fetch runs even when the cache seed already
        //    filled this range: the cache is id-keyed (arbitrary order), but the
        //    server applies the query's ordering, so the on-open fetch must
        //    actually hit the server to replace the seed with ordered rows —
        //    otherwise the grid shows cache order until a manual refresh.
        if !two_pass
            && dio.lens.defaults.refresh_on_open
            && dio.lens.callbacks.on_load_chunk.is_some()
        {
            let range = initial_range.unwrap_or(0..page_size);
            enqueue_viewport(
                &state,
                ViewportRequest {
                    range,
                    force_load: true,
                },
            );
        }

        let scenery: Arc<dyn TableScenery> = Arc::new(TableSceneryImpl {
            inner: state,
            _guard: SceneryGuard {
                tasks: vec![reactor_handle, viewport_handle],
            },
        });

        // Publish to the dedup registry. If a concurrent open won the race for
        // this key, `register_table_scenery` returns the winner and our
        // `scenery` drops here — its guard aborts the redundant tasks.
        Ok(dio.register_table_scenery(key, scenery))
    }
}
