pub mod config;
mod lens;
mod seed;

use std::future::Future;
use std::sync::Arc;

use cucumber::World;
use tempfile::TempDir;
use tokio::sync::{Mutex, Notify, broadcast};
use vantage_core::Result;
use vantage_diorama::{CacheBackend, CacheTable, Dio, DioEvent, Lens, MemoryCache, TableScenery};
use vantage_vista::Vista;

use super::backend::BackendKind;
use super::spies::Spies;
use super::sqlite_runtime::dispatch;
use config::LensBuilderState;

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
