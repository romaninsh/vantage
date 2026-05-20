use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use cucumber::World;
use tempfile::TempDir;
use tokio::sync::{Mutex, Notify, broadcast};
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{ChangeEvent, Dio, DioEvent, Lens, TableScenery, WriteOp};
use vantage_vista::Vista;

use super::backend::BackendKind;
use super::spies::Spies;

#[derive(Clone, Copy, Default, Debug)]
pub enum OnWriteMode {
    #[default]
    Unset,
    /// Counter-only — proves the worker fired, nothing else.
    Pass,
    /// Always errors — the `WriteFailed` event-bus path.
    Error,
    /// Mirror: apply the op to both master and cache, so subsequent
    /// facade reads see fresh data without round-tripping the upstream.
    Mirror,
}

#[derive(Clone, Copy, Default, Debug)]
pub enum OnEventMode {
    #[default]
    Unset,
    /// On `ChangeEvent::Updated { id, new: Some(rec) }`, forward to
    /// `dio.patched(id, rec)` so the cache + bus reflect the upstream
    /// change. Other variants are counted but otherwise ignored.
    PatchedFromUpdate,
}

#[derive(Clone, Copy, Default, Debug)]
pub enum TotalProviderKind {
    #[default]
    Unset,
    /// Calls `dio.master().get_count()` and returns it.
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
    pub on_write_mode: OnWriteMode,
    pub on_event_mode: OnEventMode,
    pub total_provider_kind: TotalProviderKind,
    pub on_load_chunk_kind: OnLoadChunkKind,
    pub register_on_refresh: bool,
    /// When present, the `on_start` closure awaits this Notify before
    /// running its body. Lets a test pin "make_dio is/isn't waiting on
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
    /// callback after `make_dio` has returned.
    pub on_start_gate: Option<Arc<Notify>>,
    /// Captured before dropping the Dio so the test can `await` clean
    /// worker exit — see scenario 9.
    pub worker_handle: Option<tokio::task::JoinHandle<()>>,
    /// Spawned `make_dio` future when a scenario needs to assert
    /// "pending" vs "complete" — see scenario 1.
    pub pending_dio: Option<tokio::task::JoinHandle<Result<Dio>>>,
    /// Opened by the `the table scenery is opened` step; subsequent
    /// generation assertions read from `scenery.subscribe()`.
    pub scenery: Option<Arc<dyn TableScenery>>,
    /// Multi-dio scenarios: a single Lens producing several Dios bound
    /// to different masters, each claiming its own cache table.
    pub named_masters: std::collections::HashMap<String, Vista>,
    pub named_dios: std::collections::HashMap<String, Dio>,
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
            named_masters: std::collections::HashMap::new(),
            named_dios: std::collections::HashMap::new(),
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
}

impl LensBuilderState {
    /// Materialise the configured Lens. Closures clone the spy counters so
    /// each callback invocation lands in the matching `AtomicU64`.
    pub fn build(&self, cache_path: std::path::PathBuf, spies: &Spies) -> Result<Arc<Lens>> {
        let mut b = Lens::new().cache_at(cache_path);

        // on_start — legacy "copy master" mode kept for v1 features;
        // v2 features select a variant via `on_start_load_kind`.
        let load_kind = match self.on_start_load_kind {
            OnStartLoad::Unset if self.on_start_load_master => OnStartLoad::AllRows,
            other => other,
        };

        if !matches!(load_kind, OnStartLoad::Unset) {
            let counter = spies.on_start.clone();
            let master_list_counter = spies.master_list_calls.clone();
            let gate = self.on_start_gate.clone();
            b = b.on_start(move |dio| {
                let dio = dio.clone();
                let counter = counter.clone();
                let master_list_counter = master_list_counter.clone();
                let gate = gate.clone();
                let load_kind = load_kind;
                async move {
                    if let Some(n) = gate.as_ref() {
                        n.notified().await;
                    }
                    counter.fetch_add(1, Ordering::SeqCst);
                    master_list_counter.fetch_add(1, Ordering::SeqCst);
                    let rows = dio.master().list_values().await?;
                    let rows: indexmap::IndexMap<_, _> = match load_kind {
                        OnStartLoad::AllRows | OnStartLoad::Unset => rows,
                        OnStartLoad::FirstN(n) => rows.into_iter().take(n).collect(),
                    };
                    dio.cache().insert_values(rows).await
                }
            });
        }

        if let Some(blocking) = self.on_start_blocking {
            b = b.on_start_blocking(blocking);
        }

        if let Some(interval) = self.refresh_every {
            b = b.refresh_every(interval);
        }

        if self.register_on_refresh {
            let counter = spies.on_refresh.clone();
            b = b.on_refresh(move |_dio| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            });
        }

        match self.on_write_mode {
            OnWriteMode::Unset => {}
            OnWriteMode::Pass => {
                let counter = spies.on_write.clone();
                b = b.on_write(move |_dio, _op| {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                });
            }
            OnWriteMode::Error => {
                let counter = spies.on_write.clone();
                b = b.on_write(move |_dio, _op| {
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        Err(vantage_core::error!("on_write rejected"))
                    }
                });
            }
            OnWriteMode::Mirror => {
                use vantage_dataset::traits::WritableValueSet;
                let counter = spies.on_write.clone();
                b = b.on_write(move |dio, op| {
                    let dio = dio.clone();
                    let counter = counter.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        match op {
                            WriteOp::Insert { id, record } => {
                                dio.master().insert_value(&id, &record).await?;
                                dio.cache().insert_value(&id, &record).await?;
                            }
                            WriteOp::Replace { id, record } => {
                                dio.master().replace_value(&id, &record).await?;
                                dio.cache().insert_value(&id, &record).await?;
                            }
                            WriteOp::Patch { id, partial } => {
                                dio.master().patch_value(&id, &partial).await?;
                                // Read-merge-write on the cache so consumers
                                // see the same merged record they'd get from
                                // a fresh master read. `cache.insert_value`
                                // is a full replace in redb, so an unmerged
                                // write would drop columns absent from the
                                // partial.
                                let mut merged = dio
                                    .cache()
                                    .get_value(&id)
                                    .await?
                                    .unwrap_or_default();
                                for (k, v) in &partial {
                                    merged.insert(k.clone(), v.clone());
                                }
                                dio.cache().insert_value(&id, &merged).await?;
                            }
                            WriteOp::Delete { id } => {
                                dio.master().delete(&id).await?;
                                dio.cache().delete_value(&id).await?;
                            }
                            WriteOp::DeleteAll => {
                                dio.master().delete_all().await?;
                                dio.cache().clear().await?;
                            }
                        }
                        Ok(())
                    }
                });
            }
        }

        match self.on_event_mode {
            OnEventMode::Unset => {}
            OnEventMode::PatchedFromUpdate => {
                let counter = spies.on_event.clone();
                b = b.on_event(move |dio, evt| {
                    let counter = counter.clone();
                    let dio = dio.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        if let ChangeEvent::Updated { id, new: Some(rec) } = evt {
                            dio.patched(id, rec).await?;
                        }
                        Ok(())
                    }
                });
            }
        }

        // total_provider — runs `dio.master().get_count()` once per
        // scenery open.
        match self.total_provider_kind {
            TotalProviderKind::Unset => {}
            TotalProviderKind::Master => {
                b = b.total_provider(move |dio| {
                    let dio = dio.clone();
                    async move {
                        let n = dio.master().get_count().await?;
                        Ok(n as usize)
                    }
                });
            }
            TotalProviderKind::MasterRecorded => {
                let counter = spies.total_provider.clone();
                b = b.total_provider(move |dio| {
                    let counter = counter.clone();
                    let dio = dio.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        let n = dio.master().get_count().await?;
                        Ok(n as usize)
                    }
                });
            }
        }

        // on_load_chunk — fetches the requested range from the master
        // and streams rows through the ChunkSink.
        match self.on_load_chunk_kind {
            OnLoadChunkKind::Unset => {}
            OnLoadChunkKind::PullFromMaster => {
                let counter = spies.on_load_chunk.clone();
                let master_list_counter = spies.master_list_calls.clone();
                let last_range = spies.last_load_chunk_range.clone();
                b = b.on_load_chunk(move |dio, range, sink| {
                    let counter = counter.clone();
                    let master_list_counter = master_list_counter.clone();
                    let last_range = last_range.clone();
                    let dio = dio.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        *last_range.lock().await = Some(range.clone());
                        master_list_counter.fetch_add(1, Ordering::SeqCst);
                        let all = dio.master().list_values().await?;
                        let slice: Vec<_> = all.into_iter().collect();
                        let start = range.start.min(slice.len());
                        let end = range.end.min(slice.len());
                        for (offset, (id, rec)) in slice[start..end].iter().enumerate() {
                            sink.push(start + offset, id.clone(), rec.clone()).await?;
                        }
                        Ok(())
                    }
                });
            }
            OnLoadChunkKind::RecordCalls => {
                let counter = spies.on_load_chunk.clone();
                let last_range = spies.last_load_chunk_range.clone();
                b = b.on_load_chunk(move |_dio, range, _sink| {
                    let counter = counter.clone();
                    let last_range = last_range.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        *last_range.lock().await = Some(range);
                        Ok(())
                    }
                });
            }
            OnLoadChunkKind::AlwaysError => {
                let counter = spies.on_load_chunk.clone();
                let last_range = spies.last_load_chunk_range.clone();
                b = b.on_load_chunk(move |_dio, range, _sink| {
                    let counter = counter.clone();
                    let last_range = last_range.clone();
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                        *last_range.lock().await = Some(range);
                        Err(vantage_core::error!("on_load_chunk rejected"))
                    }
                });
            }
        }

        if let Some(enabled) = self.refresh_on_open {
            b = b.refresh_on_open(enabled);
        }

        let lens = b.build().map_err(|e| vantage_core::error!(e.to_string()))?;
        Ok(Arc::new(lens))
    }
}
