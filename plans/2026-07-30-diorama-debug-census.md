# Diorama Debug & Census (PR 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An official per-datasource debug feature in vantage-diorama (+aggregate): a `DebugTap` that emits a curated, structured event stream — backend calls with request correlation, lens-callback timing, cache mutations, consumer census with time/memory, status transitions, column demand — under one target, `vantage_diorama::debug`, at `info` level, plus an exit summary.

**Architecture:** A `DebugTap` (datasource name + enabled flag) lives on `Lens`, set via `LensBuilder::debug_datasource(name)`. Every task the lens spawns reaches it through `DioInner::lens`. When off, behaviour and log output are byte-identical to today — the existing six `vantage_diorama::*` debug targets are untouched; the tap adds *gated `info!` lines* at curated sites, it does not re-level existing lines. `vantage-diorama-aggregate` inherits the source Dio's tap.

**Tech Stack:** Rust, tokio, tracing, libc (new, unix-only, for getrusage), redb/memory cache backends already present.

**Spec:** `/Users/rw/Work/vantage-ui/agents/specs/2026-07-30-diorama-debug-census-design.md`

## Global Constraints

- Work in a worktree: `~/Work/worktrees/vantage/diorama-debug/vantage`, branch `diorama-debug` **from `faker-shapes`** (the shape work is unmerged and this stacks on it; PR retargets main after faker-shapes merges).
- Iterate fast, ship once: NO version bumps, changelogs, cargo fmt/clippy runs mid-loop. All preflight happens once in the final task.
- Single-line commit messages. No Co-Authored-By / attribution lines anywhere.
- Fix any compiler warning you encounter, even pre-existing ones in untouched code.
- Pipe test/build output through `tee /tmp/diorama-debug-test.log`; grep the file instead of re-running.
- When `debug` is off, output must be byte-identical to before this PR: every new line must be behind `tap.enabled()`.
- Event target is always `vantage_diorama::debug`; every line carries `ds=<datasource>`; lines about a specific dio also carry `dio=<master table name>`.
- New public API gets doc comments in the crate's existing voice (see `src/stats.rs` for tone).

---

### Task 1: `debug` module — DebugTap, tapline! macro, process stats

**Files:**
- Create: `vantage-diorama/src/debug.rs`
- Modify: `vantage-diorama/src/lib.rs` (add `pub mod debug;` + re-export)
- Modify: `vantage-diorama/Cargo.toml` (unix-only `libc = "0.2"`)
- Test: unit tests inside `vantage-diorama/src/debug.rs`

**Interfaces:**
- Produces: `pub struct DebugTap` with `DebugTap::off() -> Self`, `DebugTap::for_datasource(name: impl Into<String>) -> Self`, `pub fn enabled(&self) -> bool`, `pub fn ds(&self) -> &str` (empty string when off), `Clone + Default (off) + Debug`.
- Produces: `pub struct ProcessStats { pub uptime_ms: u64, pub cpu_ms: u64, pub peak_rss_bytes: u64 }` and `pub fn process_stats() -> ProcessStats`.
- Produces: crate-internal macro `tapline!(tap, field = value, ..., "message")` — expands to `if tap.enabled() { tracing::info!(target: "vantage_diorama::debug", ds = %tap.ds(), field = value, ..., "message"); }`.

- [ ] **Step 1: Setup the worktree** (no test cycle — setup folds into this task)

```bash
mkdir -p ~/Work/worktrees/vantage/diorama-debug
cd ~/Work/vantage
git worktree add ~/Work/worktrees/vantage/diorama-debug/vantage -b diorama-debug faker-shapes
cd ~/Work/worktrees/vantage/diorama-debug/vantage
```

All subsequent paths are relative to this worktree root.

- [ ] **Step 2: Write the failing test** — append to the new `vantage-diorama/src/debug.rs` (write the tests module first; the file won't compile until Step 4, that's the failure):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_is_off_by_default_and_carries_the_datasource_name() {
        let off = DebugTap::default();
        assert!(!off.enabled());
        assert_eq!(off.ds(), "");
        let on = DebugTap::for_datasource("librarian");
        assert!(on.enabled());
        assert_eq!(on.ds(), "librarian");
    }

    #[test]
    fn process_stats_reports_nonzero_cpu_and_rss() {
        // Burn a little CPU so utime is measurable.
        let mut x = 0u64;
        for i in 0..5_000_000u64 {
            x = x.wrapping_add(i);
        }
        std::hint::black_box(x);
        let s = process_stats();
        #[cfg(unix)]
        {
            assert!(s.peak_rss_bytes > 0, "peak RSS should be measurable on unix");
            assert!(s.cpu_ms > 0, "cpu time should be nonzero after busy loop");
        }
        let _ = s.uptime_ms; // monotonic, may be 0 in a fast test — presence is enough
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p vantage-diorama debug:: 2>&1 | tee /tmp/diorama-debug-test.log`
Expected: compile FAIL — `DebugTap` not defined.

- [ ] **Step 4: Implement `vantage-diorama/src/debug.rs`**

```rust
//! The official per-datasource debug stream.
//!
//! A [`DebugTap`] is carried by every [`Lens`](crate::Lens) and reached from
//! every task the lens spawns. When enabled (one datasource opted in via
//! `debug: true`), curated events emit at `info` level under the single
//! target `vantage_diorama::debug` — visible in a default log with no
//! `RUST_LOG` required. When off, nothing is emitted and nothing is paid.
//!
//! This stream is the mechanism for demonstrating the cache's efficiency
//! and its resilience to backend faults: every master round trip, every
//! cache mutation, every consumer open/close (the *census*), every status
//! transition — attributable, correlated (`req=N`), and greppable.

use std::sync::Arc;
use std::sync::LazyLock;
use std::time::Instant;

/// Per-datasource debug switch. Cheap to clone, cheap to check.
#[derive(Debug, Clone, Default)]
pub struct DebugTap {
    /// `Some(name)` = enabled for that datasource; `None` = off.
    ds: Option<Arc<str>>,
}

impl DebugTap {
    /// The disabled tap — the default for every Lens.
    pub fn off() -> Self {
        Self { ds: None }
    }

    /// An enabled tap tagged with the datasource's name; every emitted
    /// line carries it as `ds=<name>`.
    pub fn for_datasource(name: impl Into<String>) -> Self {
        Self {
            ds: Some(Arc::from(name.into())),
        }
    }

    pub fn enabled(&self) -> bool {
        self.ds.is_some()
    }

    /// The datasource name, or `""` when the tap is off. Only meaningful
    /// inside a `tapline!` (which never fires when off).
    pub fn ds(&self) -> &str {
        self.ds.as_deref().unwrap_or("")
    }
}

/// Emit one debug-stream line, only when the tap is enabled.
///
/// `tapline!(tap, field = value, ..., "message")` — the target and the
/// `ds` field are supplied here so call sites can't drift.
macro_rules! tapline {
    ($tap:expr, $($rest:tt)*) => {
        if $tap.enabled() {
            tracing::info!(target: "vantage_diorama::debug", ds = %$tap.ds(), $($rest)*);
        }
    };
}
pub(crate) use tapline;

/// Wall/CPU/memory snapshot for census lines and the exit summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProcessStats {
    /// Milliseconds since the first `process_stats()` call in this process.
    pub uptime_ms: u64,
    /// User + system CPU time consumed by the process, in milliseconds.
    pub cpu_ms: u64,
    /// Peak resident set size, in bytes. 0 where unsupported.
    pub peak_rss_bytes: u64,
}

static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Snapshot process wall-clock, CPU time, and peak RSS.
///
/// Unix only (`getrusage`); other platforms report uptime and zeros.
pub fn process_stats() -> ProcessStats {
    let uptime_ms = PROCESS_START.elapsed().as_millis() as u64;
    #[cfg(unix)]
    {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } == 0 {
            let tv_ms = |tv: libc::timeval| tv.tv_sec as u64 * 1000 + tv.tv_usec as u64 / 1000;
            // ru_maxrss is bytes on macOS, kilobytes on Linux.
            #[cfg(target_os = "macos")]
            let peak = usage.ru_maxrss as u64;
            #[cfg(not(target_os = "macos"))]
            let peak = usage.ru_maxrss as u64 * 1024;
            return ProcessStats {
                uptime_ms,
                cpu_ms: tv_ms(usage.ru_utime) + tv_ms(usage.ru_stime),
                peak_rss_bytes: peak,
            };
        }
    }
    ProcessStats {
        uptime_ms,
        ..Default::default()
    }
}
```

In `vantage-diorama/src/lib.rs`, next to `pub mod stats;`, add:

```rust
pub mod debug;
```

and to the re-export block add `pub use debug::{DebugTap, ProcessStats, process_stats};` (match the file's existing re-export style).

In `vantage-diorama/Cargo.toml` under `[dependencies]`-adjacent sections add:

```toml
[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test -p vantage-diorama debug:: 2>&1 | tee /tmp/diorama-debug-test.log`
Expected: 2 passed.

- [ ] **Step 6: Commit**

```bash
git add vantage-diorama/src/debug.rs vantage-diorama/src/lib.rs vantage-diorama/Cargo.toml
git commit -m "diorama: DebugTap + tapline! + process_stats — the official debug stream's core"
```

---

### Task 2: Thread the tap through Lens and Dio; request sequence + per-dio census counters

**Files:**
- Modify: `vantage-diorama/src/lens/mod.rs` (Lens field ~line 42-50, LensBuilder field ~line 81-95, `new()` ~line 104, add setter)
- Modify: `vantage-diorama/src/lens/build.rs` (carry the field through `build()`)
- Modify: `vantage-diorama/src/dio/mod.rs` (`DioInner` fields ~line 116-216; `Dio` accessors)
- Modify: `vantage-diorama/src/lens/make_dio.rs` (initialize the new `DioInner` fields, ~line 41-88)
- Test: `vantage-diorama/tests/debug_tap.rs` (new — the capture-layer harness used by all later tasks)

**Interfaces:**
- Consumes: `DebugTap` from Task 1.
- Produces: `LensBuilder::debug_datasource(name: impl Into<String>) -> Self` (sets `self.debug = DebugTap::for_datasource(name)`); `Lens { pub(crate) debug: DebugTap }`; `Dio::debug_tap(&self) -> DebugTap` (public — the aggregate crate needs it); `DioInner::tap(&self) -> &DebugTap` (crate-internal, returns `&self.lens.debug`); `DioInner::next_req(&self) -> u64` (fetch_add on a new `req_seq: AtomicU64` field); new `DioInner` fields `servo_census: AtomicUsize`, `record_census: AtomicUsize` (both start 0, used in Task 3).
- Produces (test harness): `tests/debug_tap.rs` defines `fn capture() -> (tracing::subscriber::DefaultGuard, Arc<Mutex<Vec<String>>>)` installing a thread-local subscriber whose layer records `"{message} {field}={value}…"` strings for events with target `vantage_diorama::debug`, and `fn lines_containing(log: &Arc<Mutex<Vec<String>>>, needle: &str) -> Vec<String>`.

- [ ] **Step 1: Write the failing test** — create `vantage-diorama/tests/debug_tap.rs`:

```rust
//! Integration tests for the official debug stream (`vantage_diorama::debug`).

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

/// Collects every `vantage_diorama::debug` event as one flat string:
/// `"<message> ds=<..> <field>=<..> ..."` — order of fields as emitted.
struct CaptureLayer(Arc<Mutex<Vec<String>>>);

struct FlatVisitor {
    message: String,
    fields: String,
}

impl Visit for FlatVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.fields, " {}={value:?}", field.name());
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != "vantage_diorama::debug" {
            return;
        }
        let mut v = FlatVisitor {
            message: String::new(),
            fields: String::new(),
        };
        event.record(&mut v);
        self.0
            .lock()
            .unwrap()
            .push(format!("{}{}", v.message, v.fields));
    }
}

pub fn capture() -> (tracing::subscriber::DefaultGuard, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(log.clone()));
    (tracing::subscriber::set_default(subscriber), log)
}

pub fn lines_containing(log: &Arc<Mutex<Vec<String>>>, needle: &str) -> Vec<String> {
    log.lock()
        .unwrap()
        .iter()
        .filter(|l| l.contains(needle))
        .cloned()
        .collect()
}

#[tokio::test]
async fn builder_flag_reaches_the_dio_and_off_means_off() {
    use vantage_diorama::Lens;

    let lens = Lens::new()
        .cache_in_memory()
        .debug_datasource("faker-ds")
        .runtime(tokio::runtime::Handle::current())
        .build()
        .unwrap();
    let vista = vantage_vista::Vista::mock("books", vec![]); // adapt: use the mock-vista constructor the existing tests use (see tests/bdd_support/)
    let dio = lens.make_dio(vista).await.unwrap();
    assert!(dio.debug_tap().enabled());
    assert_eq!(dio.debug_tap().ds(), "faker-ds");

    let quiet = Lens::new()
        .cache_in_memory()
        .runtime(tokio::runtime::Handle::current())
        .build()
        .unwrap();
    let vista = vantage_vista::Vista::mock("books", vec![]);
    let dio = quiet.make_dio(vista).await.unwrap();
    assert!(!dio.debug_tap().enabled());
}
```

**Note to implementer:** the `Vista::mock(...)` line is a placeholder for however the crate's existing tests construct a mock master — open `vantage-diorama/tests/bdd_support/` and any `tests/chunk_*.rs` and copy their exact mock-vista helper (likely `MockShell`-based). Use that everywhere this plan says "mock vista". Also check `Lens::new()...build()` exact call shape in those tests (`build()` lives in `src/lens/build.rs`).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p vantage-diorama --test debug_tap 2>&1 | tee /tmp/diorama-debug-test.log`
Expected: compile FAIL — no method `debug_datasource`.

- [ ] **Step 3: Implement**

`src/lens/mod.rs` — add field `pub(crate) debug: crate::debug::DebugTap,` to **both** `Lens` (after `activity`) and `LensBuilder`; initialize `debug: crate::debug::DebugTap::off()` in `LensBuilder::new()`; add the setter beside `augment_workers`:

```rust
    /// Enable the official debug stream for every Dio this Lens produces.
    /// `name` is the datasource name stamped as `ds=` on every line of the
    /// `vantage_diorama::debug` target. Off by default; when off, the
    /// stream emits nothing.
    pub fn debug_datasource(mut self, name: impl Into<String>) -> Self {
        self.debug = crate::debug::DebugTap::for_datasource(name);
        self
    }
```

`src/lens/build.rs` — carry `debug: self.debug` into the constructed `Lens` (mirror how `activity` flows).

`src/dio/mod.rs` — add to `DioInner`:

```rust
    /// Monotonic per-dio request id for correlating debug-stream
    /// dispatch/return lines (`req=N`).
    pub(crate) req_seq: std::sync::atomic::AtomicU64,
    /// Live servos opened on this Dio — census bookkeeping only.
    pub(crate) servo_census: std::sync::atomic::AtomicUsize,
    /// Live record sceneries opened on this Dio — census bookkeeping only.
    pub(crate) record_census: std::sync::atomic::AtomicUsize,
```

with crate-internal helpers on `DioInner`:

```rust
    pub(crate) fn tap(&self) -> &crate::debug::DebugTap {
        &self.lens.debug
    }

    pub(crate) fn next_req(&self) -> u64 {
        self.req_seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
```

and the public accessor on `Dio` (near `diagnostics()`):

```rust
    /// The debug tap this Dio inherited from its Lens. Enabled when the
    /// datasource opted into the `vantage_diorama::debug` stream.
    pub fn debug_tap(&self) -> crate::debug::DebugTap {
        self.inner.lens.debug.clone()
    }
```

`src/lens/make_dio.rs` — initialize the three new fields (`AtomicU64::new(0)` / `AtomicUsize::new(0)`) where `DioInner` is constructed (~line 41-88).

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test -p vantage-diorama --test debug_tap 2>&1 | tee /tmp/diorama-debug-test.log`
Expected: PASS. Also run `cargo test -p vantage-diorama 2>&1 | tail -20` — no regressions.

- [ ] **Step 5: Commit**

```bash
git add -A vantage-diorama
git commit -m "diorama: thread DebugTap through Lens/DioInner with req counter and census fields"
```

---

### Task 3: Census lines on every consumer open/close

**Files:**
- Modify: `vantage-diorama/src/scenery/table/builder.rs` (after `register_table_scenery`, ~line 527)
- Modify: `vantage-diorama/src/scenery/table/mod.rs` (`SceneryGuard::drop`, ~line 214-220)
- Modify: `vantage-diorama/src/scenery/record.rs` (open + drop)
- Modify: `vantage-diorama/src/servo/mod.rs` (`Dio::servo`/`servo_new` construction site or `Servo` new + `ServoGuard::drop`, ~lines 119-135)
- Modify: `vantage-diorama/src/dio/mod.rs` (add `emit_census` helper on `DioInner`)
- Test: extend `vantage-diorama/tests/debug_tap.rs`

**Interfaces:**
- Consumes: `DioInner::{tap, servo_census, record_census}`, `Dio::live_table_scenery_count()` (exists, `dio/mod.rs:541`), `crate::debug::process_stats()`, `crate::stats::live_counts()`.
- Produces: `DioInner::emit_census(&self, verb: &'static str, kind: &'static str)` — emits one line shaped:
  `census: <kind> <verb> — N table sceneries, N record sceneries, N servos … uptime_ms=… cpu_ms=… peak_rss_mb=…`

- [ ] **Step 1: Write the failing test** — append to `tests/debug_tap.rs`:

```rust
#[tokio::test]
async fn census_lines_fire_on_scenery_open_and_drop() {
    let (_guard, log) = capture();
    let lens = /* debug lens as in previous test, on "faker-ds" */;
    let dio = /* make_dio over a mock vista named "books" */;

    let scenery = dio.table_scenery().open().await.unwrap();
    let opens = lines_containing(&log, "census: table scenery opened");
    assert_eq!(opens.len(), 1);
    assert!(opens[0].contains("dio=\"books\""), "line: {}", opens[0]);
    assert!(opens[0].contains("table_sceneries=1"), "line: {}", opens[0]);
    assert!(opens[0].contains("uptime_ms="), "census carries process stats");

    drop(scenery);
    // Guard teardown is synchronous; the census drop line is emitted from Drop.
    let closes = lines_containing(&log, "census: table scenery closed");
    assert_eq!(closes.len(), 1);
}
```

(Adapt the two `/* … */` to the helper shape settled in Task 2. If `table_scenery()` requires more setup for a mock vista, copy the minimal open from `tests/chunk_basic.rs` or nearest equivalent.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p vantage-diorama --test debug_tap census 2>&1 | tee /tmp/diorama-debug-test.log`
Expected: FAIL — 0 census lines.

- [ ] **Step 3: Implement**

On `DioInner` (in `src/dio/mod.rs`):

```rust
    /// One census line: who is consuming this Dio right now, and what the
    /// process costs. Emitted on every consumer open/close when the tap is
    /// enabled.
    pub(crate) fn emit_census(&self, kind: &'static str, verb: &'static str) {
        let tap = self.tap();
        if !tap.enabled() {
            return;
        }
        let sceneries = {
            let mut map = self.table_sceneries.lock().unwrap();
            map.retain(|_, weak| weak.strong_count() > 0);
            map.len()
        };
        let records = self.record_census.load(std::sync::atomic::Ordering::Relaxed);
        let servos = self.servo_census.load(std::sync::atomic::Ordering::Relaxed);
        let p = crate::debug::process_stats();
        crate::debug::tapline!(
            tap,
            dio = %self.master.read().unwrap().name(),
            table_sceneries = sceneries,
            record_sceneries = records,
            servos,
            uptime_ms = p.uptime_ms,
            cpu_ms = p.cpu_ms,
            peak_rss_mb = p.peak_rss_bytes / (1024 * 1024),
            "census: {kind} {verb}",
        );
    }
```

(If `table_sceneries` pruning duplicates `live_table_scenery_count()` internals, call that instead — read `dio/mod.rs:541` and reuse.)

Call sites — each is one line plus, for drops, whatever weak-ref the guard needs:
1. `builder.rs` immediately after `register_table_scenery(...)` (~line 527): `dio.inner.emit_census("table scenery", "opened");`
2. `SceneryGuard` (`scenery/table/mod.rs:194-220`): add a `dio: WeakDio` field (populated at open), and in `Drop` after aborting tasks: `if let Some(inner) = self.dio.upgrade() { inner.emit_census("table scenery", "closed"); }` — check `WeakDio`'s actual upgrade signature at `dio/mod.rs:82`.
3. `record.rs`: increment `record_census` where the scenery is constructed, `emit_census("record scenery", "opened")`; decrement + emit `"closed"` in its guard/drop (find the `Tally::record_scenery()` field — mirror its placement exactly).
4. `servo/mod.rs`: increment `servo_census` in `Servo` construction (it holds a strong `dio: Dio` — `servo/mod.rs:119`), emit `"servo opened"`; decrement + emit in `ServoGuard::drop` (`:127-135`; the guard needs a `WeakDio` — do not give it a strong one, servos already hold the strong ref on `Servo` itself and the guard must not extend the Dio's life).

- [ ] **Step 4: Run tests, verify pass + no regressions**

Run: `cargo test -p vantage-diorama 2>&1 | tee /tmp/diorama-debug-test.log && tail -5 /tmp/diorama-debug-test.log`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A vantage-diorama
git commit -m "diorama: census debug lines on every scenery/servo open and close"
```

---

### Task 4: Load lifecycle — correlated dispatch/return, CACHE hits, totals, status transitions

**Files:**
- Modify: `vantage-diorama/src/scenery/table/loader.rs` (inside `fire_chunk_load`, sites listed below)
- Modify: `vantage-diorama/src/scenery/table/state.rs` (`mark_settled` ~line 206-223; add `last_debug_state`)
- Modify: `vantage-diorama/src/scenery/table/mod.rs` (expose a `load_state_of(state)` helper if `load_state()` is only on the trait impl — see note)
- Test: extend `vantage-diorama/tests/debug_tap.rs`

**Interfaces:**
- Consumes: `DioInner::{tap, next_req}` (Task 2), `tapline!`.
- Produces (log contract, asserted by tests and later by the vantage-ui-examples harness — treat these message texts as **frozen strings**):
  - `"load dispatch"` with `req`, `dio`, `visible`, `effective`, `rows_to_fetch`, `already_cached`, `sort`, `search`, `force_load`
  - `"load return"` with `req`, `dio`, `received`, `ms`, `cached_after`
  - `"load failed"` with `req`, `dio`, `ms`, `error`
  - `"cache hit — viewport served locally"` with `dio`, `range`, `rows`
  - `"total"` with `dio`, `total`, `provenance` (one of `"stated"`, `"short-page"`, `"horizon-extended"`, `"hole-clamped"`)
  - `"state"` with `dio`, `from`, `to`, `reason` (LoadState transition)
- Produces: `TableSceneryState::last_debug_state: std::sync::Mutex<Option<LoadState>>` and `pub(crate) fn note_state(&self, dio: &DioInner, reason: &'static str)` which computes the current `LoadState`, compares to `last_debug_state`, and emits the `"state"` line on change (only when tap enabled — check the tap *before* computing, the computation takes locks).

- [ ] **Step 1: Write the failing test** — append to `tests/debug_tap.rs`. Build a debug lens whose `on_load_chunk` serves a 100-row mock (copy the chunk-callback shape from `tests/chunk_basic.rs` — it pushes `(idx, id, record)` into the sink and may `set_total`), open a scenery, drive the viewport, and assert the stream:

```rust
#[tokio::test]
async fn load_lifecycle_is_correlated_and_cache_hits_are_logged() {
    let (_guard, log) = capture();
    // Lens: cache_in_memory + debug_datasource("faker-ds") + on_load_chunk
    // serving rows 0..100 with set_total(100), viewport_debounce(1ms).
    let scenery = /* open table scenery */;

    scenery.set_viewport(0..30);
    /* wait: poll until lines_containing(&log, "load return").len() == 1, timeout 2s
       (copy the wait-for helper pattern used by tests/chunk_basic.rs) */

    let dispatch = lines_containing(&log, "load dispatch");
    let ret = lines_containing(&log, "load return");
    assert_eq!(dispatch.len(), 1);
    assert_eq!(ret.len(), 1);
    // The same req id ties them together.
    let req = dispatch[0].split("req=").nth(1).unwrap().split_whitespace().next().unwrap().to_string();
    assert!(ret[0].contains(&format!("req={req}")));
    assert!(ret[0].contains("ms="));

    // A state transition to Complete was logged.
    let states = lines_containing(&log, "state");
    assert!(states.iter().any(|l| l.contains("to=\"Complete\"")), "{states:?}");

    // Second pass over the same viewport: cache hit, no second dispatch.
    scenery.set_viewport(0..30);
    /* wait until lines_containing(&log, "cache hit").len() == 1 */
    assert_eq!(lines_containing(&log, "load dispatch").len(), 1, "no re-fetch");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p vantage-diorama --test debug_tap lifecycle 2>&1 | tee /tmp/diorama-debug-test.log`
Expected: FAIL — no such lines.

- [ ] **Step 3: Implement in `fire_chunk_load`** (`loader.rs:160-572`) — each addition sits NEXT TO the existing `tracing::debug!` for the same moment; none replaces one:

1. After the in-flight marker is set (~line 298): `let tap = dio_inner.tap().clone(); let req = tap.enabled().then(|| dio_inner.next_req());`
2. Beside the `"CACHE — served locally"` debug (~line 234-241), in the same `None =>` arm:
   `tapline!(tap_of(&dio_inner), dio = %master_name, range = ?visible, rows = visible_cached, "cache hit — viewport served locally");` — note this arm runs *before* step 1's `tap` binding; hoist `let tap = dio_inner.tap();` to just after the upgrade at ~line 165 instead, and drop the step-1 duplicate.
3. Beside the `"MASTER — fetching"` debug (~line 350): the `"load dispatch"` tapline with the fields listed in Interfaces (`sort`/`search` from the `ChunkQuery` built at ~line 363 — build `query` before the tapline).
4. Beside `"MASTER — fetch returned"` (~line 396): `"load return"` tapline (`req`, `received = pushed`, `ms`, `cached_after`).
5. Total resolution (~line 412-467): after each branch resolves, one `"total"` tapline with `provenance = "stated" | "short-page" | "horizon-extended"`; in the hole-clamp block (~line 508-533) `provenance = "hole-clamped"`.
6. In the `Err(e)` arm (~line 558): `"load failed"` tapline with `req`, `ms`, `error = %e`.
7. After `mark_settled` / generation bump (~line 543-545): `state.note_state(&dio_inner, "chunk load");`.

In `state.rs`:
- Add field `last_debug_state: Mutex<Option<super::LoadState>>` (init `None` where `TableSceneryState` is constructed — find the constructor in `builder.rs` ~line 278).
- `note_state` needs the current `LoadState`. `load_state()` is a default trait method on `TableScenery` (`mod.rs:223-236`) computed from `settled`/`total`/`rows` — extract that computation into `pub(crate) fn compute_load_state(state: &TableSceneryState) -> LoadState` in `mod.rs` and call it from both the trait method and `note_state`, so the two can never disagree.
- Call `note_state` also from `mark_settled` (settling is what flips Loading→Partial/Complete) — `mark_settled` has no `DioInner`; give `note_state` the tap another way: store a `pub(crate) debug_tap: crate::debug::DebugTap` on `TableSceneryState` at construction (cloned from the lens) plus `pub(crate) dio_name: String`, and drop the `dio: &DioInner` parameter entirely. **This is the cleaner shape — use it**: `note_state(&self, reason: &'static str)` reading `self.debug_tap` / `self.dio_name`.

- [ ] **Step 4: Run the full crate suite**

Run: `cargo test -p vantage-diorama 2>&1 | tee /tmp/diorama-debug-test.log && grep -E "test result|FAILED" /tmp/diorama-debug-test.log`
Expected: all pass; `debug_tap` tests green.

- [ ] **Step 5: Commit**

```bash
git add -A vantage-diorama
git commit -m "diorama: correlated load-lifecycle debug lines with total provenance and state transitions"
```

---

### Task 5: Cache mutation summaries

**Files:**
- Modify: `vantage-diorama/src/lens/chunk_sink.rs` (`flush()`, ~line 108-119)
- Modify: `vantage-diorama/src/scenery/table/loader.rs` (emit after `writer.flush()`, ~line 372-374)
- Modify: `vantage-diorama/src/scenery/table/builder.rs` (seed decisions ~lines 406-459)
- Test: extend `vantage-diorama/tests/debug_tap.rs`

**Interfaces:**
- Consumes: `CacheTable::count()` (`cache_backend.rs:99`), `tapline!`, `TableSceneryState::{debug_tap, dio_name}` (Task 4).
- Produces: `ChunkSink::flush_counted(&self) -> Result<FlushReport>` where `pub struct FlushReport { pub written: usize, pub cache_rows_before: i64, pub cache_rows_after: i64 }` (in `chunk_sink.rs`, `pub`); the existing `flush()` becomes `self.flush_counted().map(|_| ())` so no other caller changes.
- Produces (frozen log strings): `"cache write"` with `dio`, `written`, `new` (`after - before`), `updated` (`written - new`), `cached_rows`, `known_total`, `cached_pct`; `"cache seed"` with `dio`, `mode` (`"warm"`/`"cold"`), `rows`.

- [ ] **Step 1: Write the failing test** — append to `tests/debug_tap.rs`:

```rust
#[tokio::test]
async fn cache_writes_report_new_updated_and_percentage() {
    let (_guard, log) = capture();
    /* debug lens serving 100 rows with set_total(100), open scenery,
       set_viewport(0..30), wait for "load return" */
    let writes = lines_containing(&log, "cache write");
    assert_eq!(writes.len(), 1, "{writes:?}");
    assert!(writes[0].contains("new=30"));
    assert!(writes[0].contains("updated=0"));
    assert!(writes[0].contains("known_total=100"));
    // 30 of 100 rows → 30%.
    assert!(writes[0].contains("cached_pct=30"), "{}", writes[0]);
}
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p vantage-diorama --test debug_tap cache_writes` → FAIL.

- [ ] **Step 3: Implement.** In `chunk_sink.rs`, `flush_counted()` reads `self.cache.count().await` before and after the bulk `insert_values` (two cheap counts, and only the *report* is tap-gated — but counting is unconditional; that's fine, `count()` is an O(1)-ish metadata read on both backends — verify on `redb_cache.rs`; if it iterates, gate the whole counted path on a new `debug: bool` field cloned into the sink from the tap at construction in `loader.rs:314-319`). In `loader.rs` after flush success, emit `"cache write"`: `written` from the report, `new = after - before`, `updated = written - new`, `cached_rows = after`, `known_total` from `*state.total.read()`, `cached_pct = 100*after/total` when total > 0. In `builder.rs`, beside the existing warm/cold seed `tracing::debug!` sites (~406-423, ~428-459), add `"cache seed"` taplines with `mode` and row counts.

- [ ] **Step 4: Run full suite** — `cargo test -p vantage-diorama 2>&1 | tee /tmp/diorama-debug-test.log` → all pass.

- [ ] **Step 5: Commit**

```bash
git add -A vantage-diorama
git commit -m "diorama: cache-write and seed debug summaries with percent-cached"
```

---

### Task 6: Column demand + payload size (the wide-data detector)

**Files:**
- Modify: `vantage-diorama/src/lens/chunk_sink.rs` (collect column names + encoded size while buffering)
- Modify: `vantage-diorama/src/scenery/table/loader.rs` (emit after `"load return"`)
- Test: extend `vantage-diorama/tests/debug_tap.rs`

**Interfaces:**
- Consumes: `DioInner::demanded_columns()` (`dio/mod.rs:277-291` — returns the union across live sceneries; `None` = everything).
- Produces: `FlushReport` gains `pub columns_received: Vec<String>` (union of field names across buffered records, insertion-ordered) and `pub payload_bytes: usize` (sum of `ciborium` -encoded record sizes; compute only when the sink's `debug` flag is set — encoding costs CPU).
- Produces (frozen log string): `"columns"` with `dio`, `demanded` (`"all"` when `None`), `received_count`, `received_sample` (first 8 names + `"+N more"`), `undemanded_count`, `payload_bytes`, `rows`.

- [ ] **Step 1: Write the failing test:**

```rust
#[tokio::test]
async fn column_line_exposes_undemanded_wide_fields() {
    let (_guard, log) = capture();
    /* debug lens whose on_load_chunk pushes records with fields:
       id, name, plus extra_0001..extra_0050 (50 fat 1KB string fields).
       Scenery opened plainly (no .columns() demand → demanded = all). */
    /* viewport + wait for "load return" */
    let cols = lines_containing(&log, "columns");
    assert_eq!(cols.len(), 1);
    assert!(cols[0].contains("demanded=\"all\""));
    assert!(cols[0].contains("received_count=52"));
    assert!(cols[0].contains("payload_bytes="), "{}", cols[0]);
    // payload should be dominated by the extras: > 50KB for 1 row wouldn't
    // hold for all rows; just assert it's large.
    let bytes: usize = cols[0].split("payload_bytes=").nth(1).unwrap()
        .split_whitespace().next().unwrap().parse().unwrap();
    assert!(bytes > 50_000, "wide payload must be visible: {bytes}");
}
```

- [ ] **Step 2: Run to verify failure** — FAIL, no `"columns"` line.

- [ ] **Step 3: Implement.** In `ChunkSink::push` (when `debug`): fold each record's field names into an `IndexSet<String>`-like `Vec` (dedup by `contains` — bounded by column count, fine) and add `ciborium::ser::into_writer(&record, &mut counting_writer)` byte length (or serialize to a `Vec<u8>` and take `.len()`). In `loader.rs`, after the `"load return"` tapline, when tap enabled: `let demanded = dio_inner.demanded_columns();` and emit `"columns"` with `undemanded_count` = received columns not in the demanded set (0 when demanded is `None`? No — when `None`, demand is "everything", so `undemanded_count = 0` **but** `demanded="all"`; the interesting signal for the wide test is `received_count` + `payload_bytes`, and once vantage-ui declares real demand, `undemanded_count` becomes the leak alarm).

- [ ] **Step 4: Run full suite** — all pass.

- [ ] **Step 5: Commit**

```bash
git add -A vantage-diorama
git commit -m "diorama: column-demand debug line with payload bytes — the wide-data detector"
```

---

### Task 7: Two-pass path (list/detail) lifecycle lines

**Files:**
- Modify: `vantage-diorama/src/scenery/table/two_pass.rs` (`run_list_page` ~line 210-263, `run_detail_for_range` ~line 561-748)
- Test: extend `vantage-diorama/tests/debug_tap.rs`

**Interfaces:**
- Consumes: `tapline!`, `TableSceneryState::{debug_tap, dio_name}`, `QueryDescriptor` accessors (`src/ops/query_descriptor.rs:14-19` — conditions/sort/offset/limit).
- Produces (frozen log strings): `"list page dispatch"` with `req`, `dio`, `offset`, `limit`, `conditions`, `sort`; `"list page return"` with `req`, `rows`, `ms`, `index_len`, `complete`; `"detail pass"` with `dio`, `requested`, `pending`, `already_complete` (beside the existing census debug at `two_pass.rs:730-737`, reusing its computed values).

- [ ] **Step 1: Write the failing test.** Build a two-pass lens (`on_list_page` + `on_load_detail` — copy the registration shape from whatever existing test exercises two-pass; search `tests/` for `on_load_detail`). Serve a 40-row list and per-id details; open, viewport `0..10`, wait for hydration; assert one `"list page dispatch"`/`"list page return"` pair with matching `req`, and at least one `"detail pass"` line with `requested=10`.

- [ ] **Step 2: Run to verify failure** — FAIL.

- [ ] **Step 3: Implement** — taplines beside the existing debug/log sites in `run_list_page` (dispatch before the callback, return after, `req` from `dio_inner.next_req()`) and in `run_detail_for_range` next to the census debug at ~line 730.

- [ ] **Step 4: Run full suite** — all pass.

- [ ] **Step 5: Commit**

```bash
git add -A vantage-diorama
git commit -m "diorama: two-pass list/detail debug lines with query descriptors"
```

---

### Task 8: Exit summary — `stats::debug_summary_lines()`

**Files:**
- Modify: `vantage-diorama/src/stats.rs`
- Test: unit test in `stats.rs` tests module

**Interfaces:**
- Consumes: `fetch_stats()`, `live_counts()`, `crate::debug::process_stats()`.
- Produces: `pub fn debug_summary_lines() -> Vec<String>` — a rendered block, one string per line, no trailing newlines:
  - header: `"— diorama session summary —"`
  - per table (busiest first): `"{table}: {fetches} fetches ({repeats} repeats), {rows_received} rows ({rows_redundant} redundant), {ms_total}ms total, {ms_max}ms max"`
  - live: `"live: {dios} dios, {table_sceneries} table sceneries, {record_sceneries} record sceneries, {servos} servos"`
  - process: `"process: uptime {uptime_ms}ms, cpu {cpu_ms}ms, peak rss {peak_rss_mb}MB"`
- Also: `pub fn emit_debug_summary()` — logs each line via `tracing::info!(target: "vantage_diorama::debug", "{line}")` (unconditional — the *embedder* decides to call it, there is no lens in scope here; vantage-ui will call it only when some datasource had `debug: true`).

- [ ] **Step 1: Write the failing test** — in `stats.rs` tests module:

```rust
    #[test]
    fn summary_renders_ledger_live_and_process_lines() {
        reset_fetch_stats();
        record_fetch("book", &(0..20), 20, 0, 12);
        record_fetch("book", &(0..20), 20, 20, 9);
        let lines = debug_summary_lines();
        assert!(lines[0].contains("session summary"));
        let book = lines.iter().find(|l| l.starts_with("book:")).unwrap();
        assert!(book.contains("2 fetches (1 repeats)"), "{book}");
        assert!(book.contains("(20 redundant)"), "{book}");
        assert!(lines.iter().any(|l| l.starts_with("live:")));
        assert!(lines.iter().any(|l| l.starts_with("process:")));
    }
```

- [ ] **Step 2: Run to verify failure** — `cargo test -p vantage-diorama stats:: 2>&1 | tee /tmp/diorama-debug-test.log` → compile FAIL.

- [ ] **Step 3: Implement** exactly per the Interfaces shape (plain `format!` over the three sources).

- [ ] **Step 4: Run** — pass. (Note: tests in this file share the global `LEDGER`; `repeats_and_waste_are_counted_per_range` already calls `reset_fetch_stats()` first, and so does this one — but they can interleave across threads. If the new test flakes against the existing one, mark BOTH with `serial` semantics the way the crate handles it elsewhere, or assert on the `book` table only, which the other test never touches — the plan's test already does the latter; keep table names disjoint and drop any assertion on overall ordering.)

- [ ] **Step 5: Commit**

```bash
git add vantage-diorama/src/stats.rs
git commit -m "diorama: debug_summary_lines — the end-of-session cache evidence block"
```

---

### Task 9: Aggregate inheritance

**Files:**
- Modify: `vantage-diorama-aggregate/src/lens.rs` (`derive` ~line 192-230, `value` ~line 127-157)
- Modify: `vantage-diorama-aggregate/src/engine.rs` (`run` ~line 72-131, `recompute` ~line 199-233)
- Test: `vantage-diorama-aggregate/tests/` — extend the existing derive/engine test file (find the one exercising `AggregateLens::derive` over a source dio; add alongside).

**Interfaces:**
- Consumes: `Dio::debug_tap()` (Task 2, public), `vantage_diorama::debug::DebugTap` (re-exported — confirm `vantage-diorama-aggregate` can name it; add to diorama's re-exports if Task 1 missed it).
- Produces (frozen log strings, target `vantage_diorama::debug`, `ds` from the source dio's tap): `"aggregate recompute"` with `aggregate` (name), `trigger` (Debug-formatted `DioEvent` variant name or `"initial"`/`"debounce-trailing"`), `rows_in`, `rows_out`, `ms`, `unchanged` (bool — output equal, publish skipped).

- [ ] **Step 1: Write the failing test.** Copy the smallest existing `derive` test in the aggregate crate; enable `debug_datasource("agg-ds")` on the *source* lens; install the same `CaptureLayer` harness (copy the ~50-line helper from `vantage-diorama/tests/debug_tap.rs` into this crate's test — crates don't share test helpers); assert that after the source dio loads rows, at least one `"aggregate recompute"` line appears with `rows_out` and `unchanged=false`, and that with a non-debug source lens there are zero lines.

- [ ] **Step 2: Run to verify failure** — `cargo test -p vantage-diorama-aggregate 2>&1 | tee /tmp/diorama-debug-test.log` → FAIL.

- [ ] **Step 3: Implement.** The engine's spawn (`engine.rs:47`) receives the source `Dio` — clone its tap into the loop; in `recompute` (~line 199-233) time the compute, capture rows in/out and the equality-skip decision (~line 226-228), and emit the tapline. Thread a `trigger: &str` from the event match in `run` (~line 84-129). The `tapline!` macro is `pub(crate)` to diorama — the aggregate crate can't use it; write the `if tap.enabled() { tracing::info!(target: "vantage_diorama::debug", ds = %tap.ds(), ...) }` form inline (two sites), or export a public `DebugTap::line(...)` — prefer inline, two sites don't justify API.

- [ ] **Step 4: Run** — `cargo test -p vantage-diorama-aggregate` all pass.

- [ ] **Step 5: Commit**

```bash
git add -A vantage-diorama-aggregate
git commit -m "aggregate: recompute debug lines inherited from the source dio's tap"
```

---

### Task 10: Docs, preflight, PR

**Files:**
- Modify: `vantage-diorama/ARCHITECTURE.md` (add a "Debug stream" section: the tap, the target, the frozen message strings, the census, the summary)
- Modify: `vantage-diorama/CHANGELOG.md`, `vantage-diorama-aggregate/CHANGELOG.md`, both `Cargo.toml` versions
- Modify: `vantage-diorama/src/debug.rs` (module doc — ensure the frozen-string contract is listed: `load dispatch`, `load return`, `load failed`, `cache hit — viewport served locally`, `cache write`, `cache seed`, `total`, `state`, `columns`, `census: …`, `list page dispatch`, `list page return`, `detail pass`, `aggregate recompute`, `— diorama session summary —`)

- [ ] **Step 1: Docs.** Write the ARCHITECTURE.md section and the debug.rs module-doc contract list. Present-state voice only (what the stream is, not how it evolved).

- [ ] **Step 2: Full preflight.**

```bash
cargo test -p vantage-diorama -p vantage-diorama-aggregate 2>&1 | tee /tmp/diorama-debug-test.log
cargo +<CI toolchain version> clippy -p vantage-diorama -p vantage-diorama-aggregate --all-targets 2>&1 | tee /tmp/diorama-debug-clippy.log
cargo fmt -p vantage-diorama -p vantage-diorama-aggregate
```

(Look up the CI toolchain version the way previous PRs did — check `.github/workflows/` for the pinned version; run clippy with `cargo +<that version>`.) Fix everything, including pre-existing warnings encountered.

- [ ] **Step 3: Version bumps + changelogs.** Bump minor (new public API): `vantage-diorama` and `vantage-diorama-aggregate`, one changelog entry each — no anthropomorphic openers, describe the feature in present-state.

- [ ] **Step 4: Commit + PR.**

```bash
git add -A
git commit -m "diorama: changelog and version bumps for the debug stream"
git push -u origin diorama-debug
gh pr create --base faker-shapes --title "diorama: official per-datasource debug stream (DebugTap, census, exit summary)" --body "<summary of the feature, the frozen log contract, and a pointer to the spec>"
```

(Base is `faker-shapes` while it's unmerged; retarget to `main` if it merges first. NO attribution footer in the PR body.) Then STOP — per the worktree workflow, wait for review/merge before starting PR 2 (vantage-ui).

---

## Self-review notes

- Spec coverage: backend calls (T4, T7), lens callback timing (T4/T7 via req+ms; `on_start`/`on_refresh` timing rides the census/seed lines — if review wants explicit start/refresh lines, they are two more taplines in `make_dio.rs:100-121` and the refresh loop), cache state (T5), census+time/memory (T1/T3), status transitions (T4), column demand (T6), aggregate (T9), exit summary (T8), off-means-identical (global constraint, enforced by `tapline!`).
- Driver-level SQL/URL lifting (`vantage-sql`, `vantage-api-client`) is deliberately **not** in PR 1 — it needs the vantage-ui flag plumbing to be reachable and lands with PR 2's wiring or as a follow-up in this repo, per the spec's "same flag lifts them there".
- Frozen strings are the contract the PR-3 BDD harness greps; changing one after PR 3 lands is a breaking change to the test suite.
