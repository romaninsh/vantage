//! The Lens a scenario's Givens describe, before it is built.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::Notify;

use crate::bdd_support::spies::Spies;

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
