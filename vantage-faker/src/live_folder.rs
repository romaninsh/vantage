//! LiveFolder sim — a synthetic, constantly-mutating multi-layer log tree.
//!
//! Models a "live" log folder structure that grows in real time:
//!
//! - `{date}/access_logs_{HH}/chunk_{NN}.log` — high-volume access log. The
//!   active chunk's size bumps every second by `requests_per_sec ×
//!   bytes_per_request` bytes; when it crosses `chunk_threshold`, a new
//!   chunk starts (the old one stays). Default sized for ~100 chunks/hour.
//! - `{date}/error_logs/{HH:MM:SS}-errors.log` — rare. Each second has an
//!   `error_pct_per_sec` chance of producing one timestamped file with a
//!   random "backtrace" size.
//! - `{date}/events/{event_type}.log` — ten event types, each with its own
//!   1–10% per-second probability of bumping its file by 2000–4000 bytes.
//!   The file is created the first time its event fires.
//!
//! Everything is in memory — no real files. Each folder and file carries
//! `created` / `modified`; modifying a file touches its parent folder (and
//! ancestors up to the root) so a parent reflects its newest child.
//!
//! Two Vistas come out of one shared run loop:
//!
//! - **Listing** ([`LiveFolderSim::listing_vista`]): one row per child of a
//!   given path — `{name, kind, size, created, modified}`. Built lazily for
//!   any path the caller asks for; patched in place on each tick via
//!   `ChangeEvent`s so a subscribed Dio doesn't re-list.
//! - **Folder size** ([`LiveFolderSim::size_vista`]): `{path, size,
//!   file_count}`, **get-only** (no list). Fetched with 100ms–1s latency
//!   scaled by file count — exactly the slow-get shape debounce tests need.
//!
//! `backfill` replays the algorithm at full speed from `now − backfill` to
//! `now` on sim startup, seeding the tree before real-time ticks begin.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::interval;
use vantage_diorama::ChangeEvent;
use vantage_vista::Vista;

use crate::live_folder::listing_shell::FolderListingShell;
use crate::live_folder::size_shell::FolderSizeShell;
use crate::live_folder::tree::simulate_second;

mod listing_shell;
mod size_shell;
pub mod tree;

pub use tree::{Entry, EntryKind, Tree, format_ts};

/// Broadcast backlog before a lagged subscriber must resync via `list`.
const EVENT_CAPACITY: usize = 1024;
/// Real-time cadence: one `simulate_second` + one listing sync per tick.
const LOOP_TICK: Duration = Duration::from_secs(1);

/// Per-second event-type probabilities (percent). `(name,
/// percent_chance_per_second)` — the loop draws `rand(0,100) < pct` to fire
/// one occurrence, which bumps `<name>.log` by 2000–4000 bytes.
pub const EVENT_TYPES: &[(&str, u32)] = &[
    ("user_signup", 1),
    ("payment_succeeded", 2),
    ("payment_failed", 1),
    ("page_view", 10),
    ("api_call", 8),
    ("search_query", 6),
    ("file_upload", 3),
    ("email_sent", 4),
    ("notification", 5),
    ("error_reported", 1),
];

/// Lens push mode — how a subscribed Dio hears about changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushMode {
    /// Periodic refresh (default 1s) re-lists from the master vista. Simpler,
    /// tolerates dropped events, but a fixed latency floor.
    Poll,
    /// Forward every `ChangeEvent` from the sim into the Dio's `on_event`.
    /// Sub-frame latency, but a dropped event sticks until the next refresh.
    Notify,
}

/// Configuration for a [`LiveFolderSim`].
#[derive(Clone, Debug)]
pub struct LiveFolderConfig {
    /// Access-log requests per virtual second. Multiplied by one
    /// `bytes_per_request` draw to get the per-tick chunk size bump.
    pub requests_per_sec: u64,
    /// Range of bytes per request (one random draw per tick). Defaults to a
    /// 60–100 char access-log line.
    pub bytes_per_request: (u64, u64),
    /// When the active chunk's size meets or exceeds this, the next tick
    /// rolls a new `chunk_NN.log`. Default sized for ~100 chunks/hour.
    pub chunk_threshold: u64,
    /// Percent chance (0.0–100.0) of producing one error file per second.
    /// Fractional values (e.g. `0.1`) are supported — drawn against a
    /// 0.01%-precision roll.
    pub error_pct_per_sec: f64,
    /// Random backtrace size range for an error file, bytes.
    pub error_size: (u64, u64),
    /// Random size bump range for an event occurrence, bytes.
    pub event_bump: (u64, u64),
    /// Backfill window. The loop runs at full speed from `now − backfill`
    /// to `now` on construction before real-time ticks begin. `ZERO` skips.
    pub backfill: Duration,
}

impl Default for LiveFolderConfig {
    fn default() -> Self {
        // Default sized for ~100 chunks/hour at 100 req/s × 60–100 bytes:
        // avg 80 bytes × 100 req × 36 sec ≈ 288_000 bytes per chunk.
        Self {
            requests_per_sec: 100,
            bytes_per_request: (60, 100),
            chunk_threshold: 288_000,
            error_pct_per_sec: 1.0,
            error_size: (500, 3_000),
            event_bump: (2_000, 4_000),
            backfill: Duration::ZERO,
        }
    }
}

// `listing_columns()` / `listing_shell()` / `entry_to_record()` lived here
// before — they've moved into `listing_shell.rs` so the listing vista is a
// single self-contained shell reading the live tree, with a `subdir` HasMany
// reference for Dio traversal.

/// A run loop + shared state for one LiveFolder simulation.
///
/// Cheap to [`Clone`] — clones share the tree and the single broadcast
/// channel. The loop stops when the last clone is dropped.
#[derive(Clone)]
pub struct LiveFolderSim {
    inner: Arc<Inner>,
    /// Single broadcast: emits [`ChangeEvent::Invalidated`] on every tick. A
    /// subscribed Dio's `on_event` callback should treat it as "re-list from
    /// the master" — every [`FolderListingShell`] reads the live tree, so a
    /// refresh picks up the latest state without per-path patching.
    events: broadcast::Sender<ChangeEvent>,
    _task: Arc<AbortOnDrop>,
}

/// Mutable per-sim state. Held under one mutex — every tick mutates the
/// tree, and every list reads it. Coarse locking is fine for a sim.
pub(crate) struct SimState {
    pub(crate) tree: Tree,
}

pub(crate) struct Inner {
    pub(crate) cfg: LiveFolderConfig,
    pub(crate) state: Mutex<SimState>,
}

impl LiveFolderSim {
    /// Build the sim, run backfill synchronously (seed the tree), then spawn
    /// the real-time loop. Must be called inside a Tokio runtime context.
    pub fn new(cfg: LiveFolderConfig) -> Self {
        let now = SystemTime::now();
        let inner = Arc::new(Inner {
            cfg: cfg.clone(),
            state: Mutex::new(SimState {
                tree: Tree::new(now),
            }),
        });

        // Backfill runs synchronously: replay every virtual second from
        // `now − backfill` to `now`, mutating the tree without broadcasting
        // (no subscribers can exist yet).
        if cfg.backfill > Duration::ZERO {
            let start = now
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                .saturating_sub(cfg.backfill.as_secs());
            let end = now
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let mut state = inner.state.lock().unwrap();
            for s in start..end {
                let when = SystemTime::UNIX_EPOCH + Duration::from_secs(s);
                simulate_second(&mut state.tree, &cfg, when);
            }
        }

        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let task = spawn_loop(inner.clone(), events.clone());
        Self {
            inner,
            events,
            _task: Arc::new(AbortOnDrop(task)),
        }
    }

    /// Build the listing vista for `path`. The returned `Vista` wraps a
    /// [`FolderListingShell`] that reads the live tree on every list, so it
    /// always reflects the current state — no per-path snapshot to sync.
    ///
    /// The broadcast sender emits [`ChangeEvent::Invalidated`] on every tick;
    /// a forwarder should map it to a re-list (the shell re-reads the tree).
    ///
    /// Path is `/`-separated; empty path = root (lists dates).
    pub fn listing_vista(
        &self,
        name: impl Into<String>,
        path: impl Into<String>,
    ) -> (Vista, broadcast::Sender<ChangeEvent>) {
        let shell = FolderListingShell::new(self.inner.clone(), path.into());
        (Vista::new(name, Box::new(shell)), self.events.clone())
    }

    /// Build the get-only folder-size vista. The underlying shell computes
    /// `(size, file_count)` on demand with simulated latency.
    pub fn size_vista(&self, name: impl Into<String>) -> Vista {
        Vista::new(name, Box::new(FolderSizeShell::new(self.inner.clone())))
    }

    /// The folder-size vista as a **dio-level augment** over a listing Dio:
    /// hydrated rows gain `{size, file_count}` lazily, keyed by their full
    /// `path`, with the size vista's file-count-scaled latency intact (the
    /// debounce showcase). The detail rides as a fixed handle — it lives in
    /// no catalog — and every listing Dio of the same sim can carry its own
    /// copy: they all read the one shared tree. Rows whose base fields move
    /// (`modified` bumps) refetch through the Dio's refresh reconciliation.
    pub fn size_augment(&self) -> vantage_diorama::Augmentation {
        vantage_diorama::Augmentation {
            detail: vantage_diorama::Detail::Fixed(std::sync::Arc::new(
                self.size_vista("folder_size"),
            )),
            source: vantage_diorama::Source::Column {
                from: "path".to_string(),
                to: None,
            },
            fetch: vantage_diorama::Fetch::PerRow,
            merge: vantage_diorama::MergeRule {
                columns: vec!["size".to_string(), "file_count".to_string()],
            },
        }
    }

    /// Snapshot the tree as a flat `path → entry` map. Used by the example
    /// CLI to render the whole tree without spinning up per-path listings.
    pub fn snapshot(&self) -> Vec<(String, Entry)> {
        let state = self.inner.state.lock().unwrap();
        let mut out = Vec::new();
        fn walk(prefix: &str, entry: &Entry, out: &mut Vec<(String, Entry)>) {
            for (name, child) in &entry.children {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                out.push((path.clone(), child.clone()));
                if child.kind == EntryKind::Folder {
                    walk(&path, child, out);
                }
            }
        }
        walk("", &state.tree.root, &mut out);
        out
    }

    /// The current config (clone).
    pub fn config(&self) -> LiveFolderConfig {
        self.inner.cfg.clone()
    }
}

/// Spawn the per-second run loop. Each tick:
/// 1. Run one virtual second of the simulation.
/// 2. Broadcast `Invalidated` so subscribed Dios re-list from the live tree.
fn spawn_loop(inner: Arc<Inner>, events: broadcast::Sender<ChangeEvent>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(LOOP_TICK);
        loop {
            ticker.tick().await;
            let now = SystemTime::now();
            {
                let mut state = inner.state.lock().unwrap();
                simulate_second(&mut state.tree, &inner.cfg, now);
            }
            // Single broadcast — every subscribed Dio hears it and re-lists.
            let _ = events.send(ChangeEvent::Invalidated);
        }
    })
}

/// Aborts the run loop when the last [`LiveFolderSim`] clone drops.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use vantage_dataset::prelude::ReadableValueSet;

    fn cfg_no_backfill() -> LiveFolderConfig {
        LiveFolderConfig {
            backfill: Duration::ZERO,
            ..LiveFolderConfig::default()
        }
    }

    #[tokio::test]
    async fn listing_vista_reads_from_the_live_tree() {
        // Use a sim with backfill so there's data before any tick.
        let cfg = LiveFolderConfig {
            backfill: Duration::from_secs(3600),
            error_pct_per_sec: 100.0, // guarantee some error files
            ..LiveFolderConfig::default()
        };
        let sim = LiveFolderSim::new(cfg);

        let (root_vista, _tx) = sim.listing_vista("root", "");
        // Root lists day folders. At least one date should exist after 1h backfill.
        let rows = root_vista.list_values().await.unwrap();
        assert!(!rows.is_empty(), "root listing should have day folders");
    }

    #[tokio::test]
    async fn size_vista_returns_none_for_unknown_path() {
        let sim = LiveFolderSim::new(cfg_no_backfill());
        let vista = sim.size_vista("sizes");
        let got = vista.get_value("does/not/exist".to_string()).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn size_vista_returns_size_for_a_real_folder() {
        let sim = LiveFolderSim::new(LiveFolderConfig {
            backfill: Duration::from_secs(120),
            ..LiveFolderConfig::default()
        });
        let vista = sim.size_vista("sizes");

        // Find any populated path from the snapshot.
        let snap = sim.snapshot();
        let any_folder = snap
            .iter()
            .find(|(_, e)| e.kind == EntryKind::Folder && !e.children.is_empty())
            .map(|(p, _)| p.clone())
            .expect("backfill produced at least one folder");

        let rec = vista
            .get_value(&any_folder)
            .await
            .unwrap()
            .expect("folder resolves");
        assert!(rec.get("size").is_some());
        assert!(rec.get("file_count").is_some());
    }

    #[tokio::test]
    async fn listing_vista_supports_subdir_traversal() {
        // `get_ref("subdir", row)` on a ymd listing must descend into the
        // child identified by `row[path]`.
        let sim = LiveFolderSim::new(LiveFolderConfig {
            backfill: Duration::from_secs(3600),
            ..LiveFolderConfig::default()
        });
        let (ymd_vista, _tx) = sim.listing_vista("ymd", "");
        let ymd_rows = ymd_vista.list_values().await.unwrap();
        let date_row = ymd_rows
            .iter()
            .find(|(_, r)| r.get("kind").and_then(|v| v.as_text()) == Some("folder"))
            .map(|(_, r)| r.clone())
            .expect("at least one date folder");

        // Traverse into the date folder.
        let sub = ymd_vista.get_ref("subdir", &date_row).expect("subdir ref");
        let sub_rows = sub.list_values().await.unwrap();
        assert!(!sub_rows.is_empty(), "date folder should list its children");
        // Each child's hidden `path` field starts with the date folder's path.
        let parent = date_row
            .get("path")
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())
            .unwrap_or_default();
        for (_, rec) in &sub_rows {
            let path = rec
                .get("path")
                .and_then(|v| v.as_text())
                .map(|s| s.to_string())
                .unwrap_or_default();
            assert!(
                path.starts_with(&parent),
                "child path {path:?} should start with parent {parent:?}"
            );
        }
    }

    #[tokio::test]
    async fn size_augment_hydrates_listing_rows_over_one_dio() {
        use std::sync::Arc;

        // A backfilled tree so the root has day folders with real content.
        let sim = LiveFolderSim::new(LiveFolderConfig {
            backfill: Duration::from_secs(1800),
            ..LiveFolderConfig::default()
        });
        let (listing, _tx) = sim.listing_vista("root", "");
        let lens = Arc::new(
            vantage_diorama::Lens::new()
                .cache_in_memory()
                .viewport_debounce(Duration::from_millis(1))
                .build()
                .expect("lens builds"),
        );
        let dio = lens.make_dio(listing).await.expect("make_dio").augment(
            // The size augment is a fixed handle — no catalog entry needed.
            Arc::new(vantage_vista_factory::VistaCatalog::new()),
            vec![sim.size_augment()],
        );
        let scenery = dio.table_scenery().open().await.expect("scenery opens");
        scenery.set_viewport(0..10);

        // The listing's own size column reports 0 on folders; the augment
        // patches the recursive size in as hydration lands (with the size
        // vista's deliberate latency).
        let mut augmented = false;
        for _ in 0..100 {
            let n = scenery.row_count();
            augmented = (0..n).filter_map(|i| scenery.row(i)).any(|row| {
                row.record.get("file_count").is_some()
                    && matches!(
                        row.record.get("size"),
                        Some(ciborium::Value::Integer(i)) if i128::from(*i) > 0
                    )
            });
            if augmented {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            augmented,
            "a folder row gained a positive size + file_count from the augment"
        );
    }
}
