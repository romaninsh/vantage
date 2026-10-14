use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use cucumber::World;
use tempfile::TempDir;
use tokio::sync::{Mutex, Notify, broadcast};
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{
    Cache, CacheBackend, CacheTable, ChangeFlash, Dio, DioEvent, FlashKind, FlashOutcome, Lens,
    Loader, LoaderCx, MemoryCache, Refresher, TableScenery, Tuning,
};
use vantage_vista::Vista;

use super::backend::BackendKind;
use super::spies::Spies;
use super::sqlite_runtime::dispatch;

#[derive(Clone, Copy, Default, Debug)]
pub enum OnFlashMode {
    #[default]
    Unset,
    /// Counter-only — proves the worker fired, nothing else.
    Pass,
    /// Always errors — the `WriteFailed` event-bus path.
    Error,
    /// Mirror: write the flash through to the master; the Dio keeps the
    /// staged cache value, so subsequent facade reads see fresh data
    /// without round-tripping the upstream.
    Mirror,
}

#[derive(Clone, Copy, Default, Debug)]
pub enum TotalProviderKind {
    #[default]
    Unset,
    /// States the master's row count.
    Master,
    /// Same as `Master` but also bumps `spies.total_provider`.
    MasterRecorded,
}

#[derive(Clone, Copy, Default, Debug)]
pub enum OnLoadChunkKind {
    #[default]
    Unset,
    /// Fetches the requested range from `master.list_values()` and
    /// pushes the slice through the `ChunkSink`.
    PullFromMaster,
    /// Records the call (bumps `spies.on_load_chunk`, stashes range)
    /// without pushing anything.
    RecordCalls,
    /// Returns `Err` without pushing anything.
    AlwaysError,
}

#[derive(Clone, Copy, Default, Debug)]
pub enum OnStartLoad {
    #[default]
    Unset,
    /// Copy every row in `master.list_values()` to the cache.
    AllRows,
    /// Copy the first N rows of `master.list_values()` to the cache.
    FirstN(usize),
}

#[derive(Default, Debug)]
pub struct LensBuilderState {
    pub refresh_every: Option<Duration>,
    pub on_start_load_master: bool,
    pub on_start_load_kind: OnStartLoad,
    pub on_start_blocking: Option<bool>,
    pub on_flash_mode: OnFlashMode,
    pub total_provider_kind: TotalProviderKind,
    pub on_load_chunk_kind: OnLoadChunkKind,
    pub register_on_refresh: bool,
    /// With `register_on_refresh`, makes the on_refresh closure return `Err`
    /// (after bumping the spy) — models a refresh that hits a server error
    /// (e.g. a 503), so the refresh-failure path can be driven deterministically.
    pub on_refresh_fails: bool,
    /// When present, the `on_start` closure awaits this Notify before
    /// running its body. Lets a test pin "the dio build is/isn't waiting on
    /// the callback" deterministically — see scenario 1/2.
    pub on_start_gate: Option<Arc<Notify>>,
    /// Default `refresh_on_open` is true in prod; v2 scenarios that
    /// pre-seed the cache via on_start and then assert specific event
    /// counts disable it so the initial fetch doesn't fire.
    pub refresh_on_open: Option<bool>,
}

#[derive(World)]
#[world(init = Self::new)]
pub struct DioramaWorld {
    pub tmp: Option<TempDir>,
    /// Shared in-memory cache backend for the scenario. One instance reused
    /// across every Lens the world builds, so multiple Dios (and Lens rebuilds)
    /// see one another's cache tables — the cross-Lens sharing the old shared
    /// redb file gave, without touching disk.
    pub mem_cache: Option<Arc<MemoryCache>>,
    pub backend: BackendKind,
    pub master: Option<Vista>,
    pub lens_builder: LensBuilderState,
    pub lens: Option<Arc<Lens>>,
    pub dio: Option<Dio>,
    pub extra_dios: Vec<Dio>,
    pub event_log: Arc<Mutex<Vec<DioEvent>>>,
    pub recorder: Option<tokio::task::JoinHandle<()>>,
    pub spies: Spies,
    pub last_error: Option<String>,
    pub sqlite_db: Option<vantage_sql::sqlite::SqliteDB>,
    /// Mirrors `lens_builder.on_start_gate` so the test can release the
    /// callback after the dio build has returned.
    pub on_start_gate: Option<Arc<Notify>>,
    /// Captured before dropping the Dio so the test can `await` clean
    /// worker exit — see scenario 9.
    pub worker_handle: Option<tokio::task::JoinHandle<()>>,
    /// Spawned dio-build future when a scenario needs to assert
    /// "pending" vs "complete" — see scenario 1.
    pub pending_dio: Option<tokio::task::JoinHandle<Result<Dio>>>,
    /// Opened by the `the table scenery is opened` step; subsequent
    /// generation assertions read from `scenery.subscribe()`.
    pub scenery: Option<Arc<dyn TableScenery>>,
    /// A second view of the same query, for the shared-rows scenarios —
    /// opening it again hands back a view of its own over the same rows.
    pub scenery2: Option<Arc<dyn TableScenery>>,
    /// Multi-dio scenarios: a single Lens producing several Dios bound
    /// to different masters, each claiming its own cache table.
    pub named_masters: std::collections::HashMap<String, Vista>,
    pub named_dios: std::collections::HashMap<String, Dio>,
    /// The Mock backend's shell, retained as a live, mutable dataset
    /// handle — its store is `Arc`-shared with the master Vista, so
    /// `source.set_field(...)` mid-scenario is observed by the next read.
    /// `None` for CSV/SQLite backends.
    pub source: Option<vantage_vista::mocks::MockShell>,
    /// Reference-traversal scenarios: the Dio produced by the most recent
    /// `dio.get_ref(...)`, and a live handle to the related-table shell so a
    /// step can take its source offline mid-scenario.
    pub traversed: Option<Dio>,
    pub ref_source: Option<vantage_vista::mocks::MockShell>,
}

impl std::fmt::Debug for DioramaWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DioramaWorld")
            .field("backend", &self.backend)
            .field("has_master", &self.master.is_some())
            .field("has_dio", &self.dio.is_some())
            .field("extra_dios", &self.extra_dios.len())
            .field("last_error", &self.last_error)
            .finish()
    }
}

impl DioramaWorld {
    async fn new() -> Self {
        Self {
            tmp: None,
            mem_cache: None,
            backend: BackendKind::default(),
            master: None,
            lens_builder: LensBuilderState::default(),
            lens: None,
            dio: None,
            extra_dios: Vec::new(),
            event_log: Arc::new(Mutex::new(Vec::new())),
            recorder: None,
            spies: Spies::default(),
            last_error: None,
            sqlite_db: None,
            on_start_gate: None,
            worker_handle: None,
            pending_dio: None,
            scenery: None,
            scenery2: None,
            named_masters: std::collections::HashMap::new(),
            named_dios: std::collections::HashMap::new(),
            source: None,
            traversed: None,
            ref_source: None,
        }
    }

    /// Drive the single-threaded paused-clock runtime forward enough
    /// for spawned tasks (write worker, refresh task, scenery reload
    /// loop, event recorder) to reach their next suspension point.
    /// 20 yields covers a multi-await pipeline: bus send → recv →
    /// callback → bus send → recorder lock → push.
    pub async fn settle(&self) {
        for _ in 0..20 {
            tokio::task::yield_now().await;
        }
    }

    /// Install a one-shot Notify into both `LensBuilderState` and the World
    /// itself so steps can release the gate later.
    pub fn install_on_start_gate(&mut self) -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        self.lens_builder.on_start_gate = Some(notify.clone());
        self.on_start_gate = Some(notify.clone());
        notify
    }

    pub fn release_on_start_gate(&self) {
        if let Some(n) = self.on_start_gate.as_ref() {
            n.notify_one();
        }
    }

    /// Drain `rx` into `self.event_log` until the receiver closes. Called by
    /// the `when the dio is created` step right after `subscribe_events`.
    pub fn start_recorder(&mut self, mut rx: broadcast::Receiver<DioEvent>) {
        let log = self.event_log.clone();
        let handle = tokio::spawn(async move {
            while let Ok(evt) = rx.recv().await {
                log.lock().await.push(evt);
            }
        });
        self.recorder = Some(handle);
    }

    pub async fn snapshot_events(&self) -> Vec<DioEvent> {
        self.event_log.lock().await.clone()
    }

    pub fn tmp_path(&mut self) -> std::path::PathBuf {
        if self.tmp.is_none() {
            self.tmp = Some(TempDir::new().expect("create tempdir"));
        }
        self.tmp.as_ref().unwrap().path().to_path_buf()
    }

    /// The scenario's shared in-memory cache backend (created on first use).
    pub fn cache_source(&mut self) -> Arc<dyn CacheBackend> {
        self.mem_cache
            .get_or_insert_with(|| Arc::new(MemoryCache::new()))
            .clone()
    }

    /// The Lens the scenario's Givens configured, over the shared cache.
    pub fn build_lens(&mut self) -> Arc<Lens> {
        let cache = self.cache_source();
        self.lens_builder
            .build(cache, &self.spies, self.backend)
            .expect("build lens")
    }

    /// Build a Dio over `master` on `lens`, seeding a paged Lens's start
    /// first, and start recording its events.
    pub async fn build_dio(&mut self, lens: &Arc<Lens>, master: Vista) -> Dio {
        self.seed_paged_start(&master).await;
        let dio = lens.dio(master).build().await.expect("dio builds");
        self.start_recorder(dio.subscribe_events());
        dio
    }

    /// The "on_start" seed of a paged Lens. A paged loader's eager start runs
    /// only for a master without `Window`, so the harness copies the start's
    /// rows into the Dio's cache table itself — the warm cache a paged view
    /// opens on — and stores a total provider's count as the remembered
    /// total.
    async fn seed_paged_start(&mut self, master: &Vista) {
        let load_kind = self.lens_builder.load_kind();
        if !self.lens_builder.pages() || matches!(load_kind, OnStartLoad::Unset) {
            return;
        }
        // The seed writes the table named after the master, which is the
        // Dio's table only while the master carries no query.
        assert!(
            master.query().is_empty(),
            "a paged seed needs an unnarrowed master; a narrowed one keys its own cache table"
        );
        self.spies.on_start.fetch_add(1, Ordering::SeqCst);
        self.spies.master_list_calls.fetch_add(1, Ordering::SeqCst);
        let bridge = self.backend == BackendKind::Sqlite;
        let rows = on_master(
            bridge,
            master.clone(),
            |m| async move { m.list_values().await },
        )
        .await
        .expect("list master");
        let total = self
            .lens_builder
            .total_provider_kind
            .count(&self.spies, rows.len());
        let table = self
            .cache_source()
            .open_table(master.name())
            .await
            .expect("open cache table");
        table
            .insert_values(load_kind.select(rows))
            .await
            .expect("seed cache");
        if let Some(total) = total {
            table
                .set_meta_total(total as u64)
                .await
                .expect("remember the total");
        }
    }

    /// The raw cache table behind `dio`, read from the shared backend.
    pub async fn cache_of(&self, dio: &Dio) -> Arc<dyn CacheTable> {
        let name = dio.diagnostics().await.expect("diagnostics").cache_table;
        self.mem_cache
            .as_ref()
            .expect("no cache backend")
            .open_table(&name)
            .await
            .expect("open cache table")
    }
}

/// Run `f` over `master` — on the sqlite io-runtime when `bridge` is set.
///
/// SQLite must bridge (see [`super::sqlite_runtime`] for why); Mock/CSV must
/// NOT — the cross-runtime suspension desynchronises the worker from the test
/// task's `settle()` / `drain_write_queue` polling and races cache assertions.
pub async fn on_master<T, F, Fut>(bridge: bool, master: Vista, f: F) -> Result<T>
where
    F: FnOnce(Vista) -> Fut,
    Fut: Future<Output = Result<T>> + Send + 'static,
    T: Send + 'static,
{
    if bridge {
        dispatch(f(master)).await
    } else {
        f(master).await
    }
}

/// Send one flash to the master: the write-through a mirroring writer does.
/// The Dio stages the change in the cache itself and keeps it on success.
async fn write_master(master: Vista, flash: ChangeFlash) -> Result<()> {
    use vantage_dataset::traits::WritableValueSet;
    let id = || flash.id().expect("the flash carries an id").to_string();
    match flash.kind() {
        FlashKind::Insert => drop(master.insert_value(id(), flash.patch()).await?),
        FlashKind::Replace => drop(master.replace_value(id(), flash.patch()).await?),
        FlashKind::Upsert => drop(master.upsert_value(&id(), flash.patch()).await?),
        FlashKind::Patch => drop(master.patch_value(id(), flash.patch()).await?),
        FlashKind::Delete => master.delete(id()).await?,
        FlashKind::Clear => master.delete_all().await?,
    }
    Ok(())
}

impl LensBuilderState {
    /// The start's row selection: the v1 "copy master" switch, or a v2 variant.
    pub fn load_kind(&self) -> OnStartLoad {
        match self.on_start_load_kind {
            OnStartLoad::Unset if self.on_start_load_master => OnStartLoad::AllRows,
            other => other,
        }
    }

    /// Whether the Lens pages windows: a chunk callback, or a total provider
    /// (a paged loader is the only place a total is stated apart from the
    /// cache's size).
    pub fn pages(&self) -> bool {
        !matches!(self.on_load_chunk_kind, OnLoadChunkKind::Unset)
            || !matches!(self.total_provider_kind, TotalProviderKind::Unset)
    }

    /// Materialise the configured Lens. Closures clone the spy counters so
    /// each callback invocation lands in the matching `AtomicU64`.
    ///
    /// The scenario's hooks map onto the 2.0 Lens as follows:
    /// - a chunk callback, or a total provider alone, becomes
    ///   `Loader::paged`. A paged Lens copies nothing at build over a master
    ///   with `Window`, so its "on_start" seed is written by
    ///   [`DioramaWorld::seed_paged_start`] before the Dio is built, and the
    ///   total provider's count is stored as the cache's remembered total
    ///   there and stated by each chunk load;
    /// - otherwise "on_start" is `Loader::eager`, with `on_start_blocking`
    ///   as `.blocking(..)`;
    /// - "on_refresh" and the refresh interval are a `Refresher`;
    /// - "on_flash" is the Lens writer;
    /// - `refresh_on_open` is a `Tuning` setting.
    pub fn build(
        &self,
        cache: Arc<dyn CacheBackend>,
        spies: &Spies,
        backend: BackendKind,
    ) -> Result<Arc<Lens>> {
        let bridge = backend == BackendKind::Sqlite;
        let mut b = Lens::builder().cache(Cache::backend(cache));

        let load_kind = self.load_kind();
        if self.pages() {
            b = b.loader(self.paged_loader(spies, bridge));
        } else if !matches!(load_kind, OnStartLoad::Unset) {
            let counter = spies.on_start.clone();
            let master_list_counter = spies.master_list_calls.clone();
            let gate = self.on_start_gate.clone();
            let mut loader = Loader::eager(move |cx: LoaderCx| {
                let counter = counter.clone();
                let master_list_counter = master_list_counter.clone();
                let gate = gate.clone();
                async move {
                    if let Some(n) = gate.as_ref() {
                        n.notified().await;
                    }
                    counter.fetch_add(1, Ordering::SeqCst);
                    master_list_counter.fetch_add(1, Ordering::SeqCst);
                    let rows = on_master(bridge, cx.master().clone(), |m| async move {
                        m.list_values().await
                    })
                    .await?;
                    cx.cache().insert_values(load_kind.select(rows)).await
                }
            });
            if let Some(blocking) = self.on_start_blocking {
                loader = loader.blocking(blocking);
            }
            b = b.loader(loader);
        }

        let refresher = if self.register_on_refresh {
            let counter = spies.on_refresh.clone();
            let fails = self.on_refresh_fails;
            Some(Refresher::new(move |_cx: LoaderCx| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    if fails {
                        return Err(vantage_core::error!("on_refresh failed (injected 503)"));
                    }
                    Ok(())
                }
            }))
        } else {
            self.refresh_every
                .map(|_| Refresher::new(|_cx: LoaderCx| async { Ok(()) }))
        };
        if let Some(mut refresher) = refresher {
            if let Some(interval) = self.refresh_every {
                refresher = refresher.every(interval);
            }
            b = b.refresher(refresher);
        }

        if !matches!(self.on_flash_mode, OnFlashMode::Unset) {
            let mode = self.on_flash_mode;
            let counter = spies.on_flash.clone();
            b = b.writer(move |cx: LoaderCx, flash: ChangeFlash| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    match mode {
                        OnFlashMode::Error => {
                            return Err(vantage_core::error!("on_flash rejected"));
                        }
                        OnFlashMode::Mirror => {
                            on_master(bridge, cx.master().clone(), |m| write_master(m, flash))
                                .await?
                        }
                        OnFlashMode::Pass | OnFlashMode::Unset => {}
                    }
                    Ok(FlashOutcome::applied())
                }
            });
        }

        if let Some(enabled) = self.refresh_on_open {
            b = b.tuning(Tuning::default().with_refresh_on_open(enabled));
        }

        b.build()
    }

    /// The paged loader for the configured chunk callback (a total provider
    /// alone pulls from the master). With a total provider, every chunk load
    /// also states the master's count.
    fn paged_loader(&self, spies: &Spies, bridge: bool) -> Loader {
        let kind = match self.on_load_chunk_kind {
            OnLoadChunkKind::Unset => OnLoadChunkKind::PullFromMaster,
            other => other,
        };
        let total_kind = self.total_provider_kind;
        let spies = spies.clone();
        Loader::paged(move |cx: LoaderCx, range, _query, sink| {
            let spies = spies.clone();
            async move {
                spies.on_load_chunk.fetch_add(1, Ordering::SeqCst);
                *spies.last_load_chunk_range.lock().await = Some(range.clone());
                match kind {
                    OnLoadChunkKind::RecordCalls => return Ok(()),
                    OnLoadChunkKind::AlwaysError => {
                        return Err(vantage_core::error!("on_load_chunk rejected"));
                    }
                    OnLoadChunkKind::PullFromMaster | OnLoadChunkKind::Unset => {}
                }
                // Scriptable-source gate: virtual-time latency, then a
                // self-clearing counted fault. Both default to 0, so an
                // unscripted scenario is unaffected.
                let latency_ms = spies.source_latency_ms.load(Ordering::SeqCst);
                if latency_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(latency_ms)).await;
                }
                if spies.source_fail_reads.load(Ordering::SeqCst) > 0 {
                    spies.source_fail_reads.fetch_sub(1, Ordering::SeqCst);
                    return Err(vantage_core::error!("source read failed (injected)"));
                }
                if spies.on_load_chunk_error_once.swap(false, Ordering::SeqCst) {
                    return Err(vantage_core::error!("on_load_chunk rejected (one-shot)"));
                }
                spies.master_list_calls.fetch_add(1, Ordering::SeqCst);
                let all = on_master(bridge, cx.master().clone(), |m| async move {
                    m.list_values().await
                })
                .await?;
                if let Some(total) = total_kind.count(&spies, all.len()) {
                    sink.set_total(total);
                }
                let slice: Vec<_> = all.into_iter().collect();
                let start = range.start.min(slice.len());
                let end = range.end.min(slice.len());
                for (offset, (id, rec)) in slice[start..end].iter().enumerate() {
                    sink.push(start + offset, id.clone(), rec.clone()).await?;
                }
                Ok(())
            }
        })
    }
}

impl OnStartLoad {
    /// The rows a start copies out of the master's listing.
    pub fn select(
        self,
        rows: indexmap::IndexMap<String, vantage_types::Record<ciborium::Value>>,
    ) -> indexmap::IndexMap<String, vantage_types::Record<ciborium::Value>> {
        match self {
            OnStartLoad::AllRows | OnStartLoad::Unset => rows,
            OnStartLoad::FirstN(n) => rows.into_iter().take(n).collect(),
        }
    }
}

impl TotalProviderKind {
    /// The total this provider states for a master of `master_rows` rows,
    /// counting the call when it records calls. `None` without a provider.
    pub fn count(self, spies: &Spies, master_rows: usize) -> Option<usize> {
        match self {
            TotalProviderKind::Unset => None,
            TotalProviderKind::Master => Some(master_rows),
            TotalProviderKind::MasterRecorded => {
                spies.total_provider.fetch_add(1, Ordering::SeqCst);
                Some(master_rows)
            }
        }
    }
}
