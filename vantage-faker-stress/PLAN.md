# vantage-faker-stress Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone stress harness that runs faker sim scenarios. It reports sim counts, CPU, memory, threads, write and event rates, and consumer lag, and gives chaos scenarios a verdict.

**Architecture:** A library-plus-binary crate `vantage-faker-stress` sits at the vantage repo root. It is standalone and listed in the workspace `exclude`.
- **Scenarios:** YAML files in the vantage-ui faker datasource shape. They load into faker tables and a `SimEngine`.
- **Sampling:** a sampler reads process stats (`sysinfo` plus a native thread count) and a new `SimEngine::stats()` once a second.
- **Event load:** per-table subscriber tasks count events, and can optionally forward them into a Dio and TableScenery to load the consumer side.
- **Reports:** printed as a live table. They can also be written as JSON and compared across faker versions.

**Tech Stack:** Rust 2024, vantage-faker (feature `rhai`), vantage-diorama, tokio, clap 4, sysinfo 0.39, serde_yaml_ng 0.10, libc.

**Spec:** `vantage-faker-stress/SPEC.md` (read it first; this plan argues from it).

**Worktree:** Do all work in `/Users/rw/Work/vantage-faker-rework`, on branch `faker/rework`. Do not touch `/Users/rw/Work/vantage`.

## Global Constraints

- The crate is standalone: an empty `[workspace]` table in its Cargo.toml, and `"vantage-faker-stress"` added to the root `Cargo.toml` `exclude` list.
- `publish = false`. Path dependencies only; no version pins on vantage crates.
- Files stay around 200 LOC or less, one responsibility each. There are no `mod.rs` files: use `foo.rs` next to `foo/`.
- Comments are for someone reading the code cold. No "we just fixed", no "for the demo", no narration.
- Commit messages are one line (`-m "..."`), with no Co-Authored-By trailer.
- The default build profile is **debug**. Numbers are relative, not absolute.
- `MAX_LIVE` is 1000 live sims per engine, and the engine rejects scenarios whose `max`es sum above it.
- Nothing asserts absolute performance numbers.
- The only behaviour change in vantage-faker is the read-only `SimEngine::stats()` and its counters. Nothing it exposes gets fixed here. Findings are recorded instead.
- CHANGELOG entries are one terse line. The line goes in the existing `## 0.7.0 — 2026-09-28` block, because PR #405 is unreleased.
- Run tests with `cargo nextest run` where available, else `cargo test`. Don't chain slow cargo commands with `&&`; run them as separate commands.

## Spec deltas (made while planning; Task 10 folds them into SPEC.md)

1. **Column format.** A column is `{ type?, faker? }`, the same as a vantage-ui table column (`faker:` holds the `ColumnGen`). It is not a bare generator map, so `note: {}` still reads as "no generator".
2. **`!include` scope.** An include resolves relative to the scenario's own directory, but may reach anywhere under `scenarios/`, so `chaos/*` can share `chaos/steady.rhai` and `warm` can reuse `lifecycle/parcel.rhai`.
3. **What a ramp step changes.** Step `N` applies a scale factor `N / base`:
   - By default `base` is the sum of every def's `max`, and the factor scales `burst`, `max` and table `count` (the same as `--scale`).
   - A scenario may set `stress.ramp: { base: <n>, sims: false }`, which scales only the table `count`s. `sweeper` uses this, because it ramps rows, not sims.
4. **Chaos verdict window.** CPU is judged on the mean of the last 3 samples of the run against the 1 s idle baseline taken before the engine starts, with 10 points of slack. The run length is the scenario's `stress.duration`.
5. **`chaos/flood` expectation.** 10k inserts end normally, since they're well under the 50M-operation budget. The finding to look for is `lagged` and the consumer's re-list, not an error.
6. **`stress.dio: true`** in a scenario turns on the Dio load, as `--dio` does. `chaos/flood` sets it.
7. **Chaos test is `#[ignore]`.** The smoke test runs every load scenario for 2 s. The chaos scenarios need their full duration to reach a verdict, so they run on request (`cargo test -- --ignored`), one at a time, because each run measures the whole process's CPU.

## Review Focus

- **Stale ids.** A script patches, sets or deletes an id that doesn't exist. Expected: no panic, no `writes` counted, and the engine carries on. Pinned in Task 1 (`stale_writes_are_not_counted`).
- **A table given no `table:` in a multi-table scenario.** Expected: the sim writes the first table, as vantage-ui does. It isn't an error. Pinned in Task 2 (`sim_without_table_uses_first_table`).
- **`!include` outside `scenarios/`, or pointing at a missing file.** Expected: a load error naming the file. Never a panic, never a read outside the root. Pinned in Task 2.
- **`--scale` with fractional values.** Expected: a non-zero value never rounds to 0, and a 0 burst stays 0. Pinned in Task 2 (`scale_keeps_nonzero_at_least_one`).
- **A ramp step whose scaled `max`es pass `MAX_LIVE`.** Expected: the ramp stops before starting that engine, with the reason in the report. It doesn't crash on the engine's validation error. Pinned in Task 6 (`ramp_stops_before_max_live`).

---

### Task 1: `SimEngine::stats()` in vantage-faker

**Files:**
- Create: `vantage-faker/src/sim/stats.rs`
- Modify: `vantage-faker/src/sim.rs` (module list and re-exports, near lines 60-75)
- Modify: `vantage-faker/src/sim/kind.rs` (the `Inner` struct)
- Modify: `vantage-faker/src/sim/builder.rs` (the `Inner { .. }` literal in `start`)
- Modify: `vantage-faker/src/sim/spawn.rs` (the `Ok(handle)` arm, around line 117)
- Modify: `vantage-faker/src/sim/current.rs` (the end of `run_sim`)
- Modify: `vantage-faker/src/sim/vocab/data.rs` (`insert`, `patch`, `set`, `delete`)
- Modify: `vantage-faker/src/sim/engine.rs` (add `stats`)
- Modify: `vantage-faker/src/lib.rs` (re-export `SimStats` next to `SimEngine`)
- Modify: `vantage-faker/CHANGELOG.md`
- Test: `vantage-faker/src/sim/tests/stats.rs`, and register `mod stats;` in `vantage-faker/src/sim/tests.rs`

**Interfaces:**
- Produces: `vantage_faker::SimStats { live: usize, spawned: u64, ended: u64, errored: u64, writes: u64 }` (`Clone, Copy, Debug, Default, PartialEq, Eq`), and `SimEngine::stats(&self) -> SimStats`.

- [ ] **Step 1: Write the failing tests**

Create `vantage-faker/src/sim/tests/stats.rs`:

```rust
//! `SimEngine::stats`: spawn, end, error and write counters.

use super::*;

#[test]
fn stats_count_spawned_ended_and_writes() {
    let script = r#"
        let id = insert(#{ who: "a" });
        patch(id, #{ step: "1" });
        set(id, "step", "2");
        sleep(seconds(10));
        delete(id);
    "#;
    let (engine, _log) =
        engine_with(vec![SimDef::new("a", "log", script).with_spawn(3, 0.0, 3)]);
    run_for(&engine, 20, 5);
    let s = engine.stats();
    assert_eq!(s.live, 0);
    assert_eq!(s.spawned, 3);
    assert_eq!(s.ended, 3);
    assert_eq!(s.errored, 0);
    assert_eq!(s.writes, 12);
}

#[test]
fn stats_count_errors_apart_from_ends() {
    let (engine, _log) = engine_with(vec![
        SimDef::new("bad", "log", r#"sleep(seconds(1)); throw "boom";"#).with_spawn(2, 0.0, 2),
        SimDef::new("good", "log", "sleep(seconds(1));").with_spawn(1, 0.0, 1),
    ]);
    run_for(&engine, 5, 1);
    let s = engine.stats();
    assert_eq!((s.spawned, s.ended, s.errored), (3, 1, 2));
}

#[test]
fn stale_writes_are_not_counted() {
    let script = r#"
        patch("nope", #{ step: "1" });
        set("nope", "step", "2");
        delete("nope");
    "#;
    let (engine, _log) =
        engine_with(vec![SimDef::new("a", "log", script).with_spawn(1, 0.0, 1)]);
    run_for(&engine, 2, 1);
    let s = engine.stats();
    assert_eq!(s.writes, 0);
    assert_eq!(s.errored, 0);
    assert_eq!(s.ended, 1);
}
```

Add `mod stats;` to the `mod` list at the top of `vantage-faker/src/sim/tests.rs`, keeping the alphabetical order (after `mod spawner;`).

- [ ] **Step 2: Run the tests and check they fail**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker && cargo test --features rhai stats_ 2>&1 | tail -20`
Expected: a compile error, `no method named stats found for struct SimEngine`.

- [ ] **Step 3: Add the counters**

Create `vantage-faker/src/sim/stats.rs`:

```rust
//! Running totals an engine keeps, read through [`SimEngine::stats`](super::SimEngine::stats).

use std::sync::atomic::{AtomicU64, Ordering};

/// A snapshot of an engine's counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimStats {
    /// Sims running now.
    pub live: usize,
    /// Sims started since the engine started, warm start included.
    pub spawned: u64,
    /// Sims whose script ran to its end, called `done()`, or stopped with the engine.
    pub ended: u64,
    /// Sims ended by a Rhai error: a thrown exception or a budget, depth or call-level limit.
    pub errored: u64,
    /// `insert`, `set`, `patch` and `delete` calls that changed a row.
    pub writes: u64,
}

#[derive(Default)]
pub(super) struct Counters {
    pub spawned: AtomicU64,
    pub ended: AtomicU64,
    pub errored: AtomicU64,
    pub writes: AtomicU64,
}

impl Counters {
    pub fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn read(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }
}
```

In `vantage-faker/src/sim.rs`, add `mod stats;` to the module list (alphabetical, after `mod spawn;`) and `pub use stats::SimStats;` next to `pub use engine::SimEngine;`.

In `vantage-faker/src/lib.rs`, change the sim re-export to:

```rust
#[cfg(feature = "rhai")]
pub use sim::{SimDef, SimEngine, SimEngineBuilder, SimStats, Spawn};
```

In `vantage-faker/src/sim/kind.rs`, add `use super::stats::Counters;` and a field on `Inner`, after `handles`:

```rust
    pub handles: Mutex<Vec<JoinHandle<()>>>,
    pub counters: Counters,
```

In `vantage-faker/src/sim/builder.rs`, inside the `Inner { .. }` literal in `start`, add `counters: Default::default(),` after `handles: Mutex::default(),`.

- [ ] **Step 4: Count spawns, ends and errors**

In `vantage-faker/src/sim/spawn.rs`, add `use super::stats::Counters;` and bump on a started thread:

```rust
    match spawned {
        Ok(handle) => {
            Counters::bump(&inner.counters.spawned);
            let mut handles = inner.handles.lock().unwrap_or_else(|e| e.into_inner());
```

In `vantage-faker/src/sim/current.rs`, add `use super::stats::Counters;` and replace the end of `run_sim` (from `let done = ...` to the end of the function) with:

```rust
    let done = CURRENT.with_borrow(|c| c.as_ref().is_some_and(|c| c.done));
    let failed = result.is_err() && !done && !inner.sched.is_stopped();
    if failed && let Err(e) = &result {
        k.report(&e.to_string());
    }
    Counters::bump(if failed {
        &inner.counters.errored
    } else {
        &inner.counters.ended
    });
}
```

- [ ] **Step 5: Count writes that changed a row**

In `vantage-faker/src/sim/vocab/data.rs`, add `use crate::sim::stats::Counters;`, then make these changes. Each write keeps its current effect on the store and its broadcast; only the counting is new.

```rust
fn wrote(c: &Current) {
    Counters::bump(&c.inner.counters.writes);
}
```

At the end of `insert`, bump before returning. Replace the `Ok(match given { .. })` expression with:

```rust
    let id = match given {
        Some(id) => {
            rec.insert(id_column, CborValue::Text(id.clone()));
            ctx.upsert_record(&id, rec);
            id
        }
        None => ctx.insert_record(rec),
    };
    wrote(c);
    Ok(id)
```

Replace `patch`, `set` and `delete` with:

```rust
fn patch(t: Option<&str>, id: &str, map: &RhaiMap) -> VerbResult<()> {
    with(|c| {
        let ctx = table(c, t)?;
        let existed = ctx.get_record(id).is_some();
        ctx.patch_record(id, &map_to_record(map));
        if existed {
            wrote(c);
        }
        Ok(())
    })
}

fn set(t: Option<&str>, id: &str, field: &str, v: &Dynamic) -> VerbResult<()> {
    with(|c| {
        let ctx = table(c, t)?;
        let existed = ctx.get_record(id).is_some();
        ctx.update_field(id, field, dynamic_to_cbor(v));
        if existed {
            wrote(c);
        }
        Ok(())
    })
}

fn delete(t: Option<&str>, id: &str) -> VerbResult<()> {
    with(|c| {
        let ctx = table(c, t)?;
        let existed = ctx.get_record(id).is_some();
        ctx.expire(id);
        if existed {
            wrote(c);
        }
        Ok(())
    })
}
```

- [ ] **Step 6: Expose `stats`**

In `vantage-faker/src/sim/engine.rs`, add `use super::stats::{Counters, SimStats};` and, after `threads()`:

```rust
    /// Live sims and the engine's running totals.
    pub fn stats(&self) -> SimStats {
        let c = &self.inner.counters;
        SimStats {
            live: self.live(),
            spawned: Counters::read(&c.spawned),
            ended: Counters::read(&c.ended),
            errored: Counters::read(&c.errored),
            writes: Counters::read(&c.writes),
        }
    }
```

- [ ] **Step 7: Run the tests and check they pass**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker && cargo test --features rhai stats_ 2>&1 | tail -20`
Expected: all 3 tests pass.

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker && cargo test --features rhai 2>&1 | tail -5`
Expected: the whole suite passes. Existing sim tests must be unaffected.

- [ ] **Step 8: CHANGELOG and commit**

Append to the `## 0.7.0 — 2026-09-28` block in `vantage-faker/CHANGELOG.md`:

```
- `SimEngine::stats` reports live sims and spawned, ended, errored and write totals.
```

```bash
cd /Users/rw/Work/vantage-faker-rework
git add vantage-faker
git commit -m "vantage-faker: SimEngine::stats counts spawns, ends, errors and writes"
```

---

### Task 2: Crate scaffold and scenario loading

**Files:**
- Modify: `Cargo.toml` (root, add `"vantage-faker-stress"` to `exclude`, after `"vantage-faker"`)
- Create: `vantage-faker-stress/Cargo.toml`
- Create: `vantage-faker-stress/src/lib.rs`
- Create: `vantage-faker-stress/src/scenario.rs` (types and loading)
- Create: `vantage-faker-stress/src/scenario/include.rs` (`!include` resolution)
- Create: `vantage-faker-stress/src/scenario/build.rs` (durations, scaling, SimDefs, FakerColumns)
- Create: `vantage-faker-stress/src/scenario/tests.rs`
- Create: `vantage-faker-stress/src/main.rs` (placeholder `fn main() {}` so the bin target builds; Task 6 replaces it)

**Interfaces:**
- Produces:
  - `scenario::Scenario { seed: Option<u64>, tables: IndexMap<String, TableSpec>, sims: IndexMap<String, SimSpec>, stress: StressSpec }`
  - `scenario::load(root: &Path, name: &str) -> Result<Scenario, String>`
  - `scenario::parse_duration(s: &str) -> Result<Duration, String>`
  - `Scenario::scaled(&self, sims: f64, rows: f64) -> Scenario`
  - `Scenario::without_warm(&self) -> Scenario`
  - `Scenario::sim_defs(&self) -> Result<Vec<SimDef>, String>`
  - `Scenario::faker_columns(&self, table: &str) -> Vec<FakerColumn>`
  - `Scenario::duration(&self) -> Result<Duration, String>`
  - `Scenario::max_total(&self) -> usize` (the sum of the resolved spawn `max`)
  - `StressSpec { duration: String, limits: Limits, ramp: RampSpec, expect: Option<Expect>, dio: bool }`
  - `Limits { cpu_pct: Option<f64>, event_lag_ms: Option<f64>, rss_mb: Option<f64> }`
  - `RampSpec { base: Option<usize>, sims: bool }`
  - `Expect { errored_min: u64, max_threads: Option<usize> }`

- [ ] **Step 1: Scaffold the crate**

In the root `Cargo.toml` `exclude` list, add `"vantage-faker-stress",` after `"vantage-faker",`.

Create `vantage-faker-stress/Cargo.toml`:

```toml
[package]
name = "vantage-faker-stress"
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
description = "Stress harness for vantage-faker sims: capacity, cost and containment"
publish = false

# Standalone: excluded from the parent workspace, like vantage-faker.
[workspace]

[[bin]]
name = "faker-stress"
path = "src/main.rs"

[dependencies]
vantage-faker = { path = "../vantage-faker", features = ["rhai"] }
vantage-diorama = { path = "../vantage-diorama" }
vantage-vista = { path = "../vantage-vista" }
vantage-core = { path = "../vantage-core" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml_ng = "0.10"
indexmap = { version = "2", features = ["serde"] }
clap = { version = "4.6", features = ["derive"] }
sysinfo = "0.39"
libc = "0.2"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tempfile = "3"
```

Create `vantage-faker-stress/src/lib.rs`:

```rust
//! Stress harness for vantage-faker sims. See README.md.

pub mod scenario;
```

Create `vantage-faker-stress/src/main.rs`:

```rust
fn main() {}
```

- [ ] **Step 2: Write the failing tests**

Create `vantage-faker-stress/src/scenario/tests.rs`:

```rust
use std::fs;
use std::path::Path;
use std::time::Duration;

use super::*;

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

const BASIC: &str = r#"
seed: 7
tables:
  ticket:
    count: 20
    columns:
      status: { faker: { pick: { values: [Open, Closed] } } }
      amount: { type: int, faker: { range: { min: 1, max: 500 } } }
      note: {}
  audit: { count: 0, columns: { what: {} } }
sims:
  churn:
    table: ticket
    script: !include churn.rhai
    clock: 10
    warm: 2h
    spawn: { burst: 3, rate: 1.5, max: 10, args: { who: bob } }
  audit: { script: "sleep(seconds(1));" }
stress:
  duration: 5s
  limits: { cpu_pct: 400 }
"#;

#[test]
fn loads_tables_sims_and_includes() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "let id = insert(#{});");
    let s = load(root.path(), "basic").unwrap();
    assert_eq!(s.seed, Some(7));
    assert_eq!(s.tables["ticket"].count, 20);
    assert_eq!(s.sims["churn"].script, "let id = insert(#{});");
    assert_eq!(s.duration().unwrap(), Duration::from_secs(5));
    assert_eq!(s.stress.limits.cpu_pct, Some(400.0));
}

#[test]
fn sim_defs_apply_vantage_ui_defaults() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let defs = load(root.path(), "basic").unwrap().sim_defs().unwrap();
    let churn = &defs[0];
    assert_eq!((churn.spawn.burst, churn.spawn.rate_per_min, churn.spawn.max), (3, 1.5, 10));
    assert_eq!(churn.clock, 10.0);
    assert_eq!(churn.warm, Some(Duration::from_secs(7200)));
    assert_eq!(churn.spawn.args["who"], "bob");
    let audit = &defs[1];
    assert_eq!((audit.spawn.burst, audit.spawn.rate_per_min, audit.spawn.max), (1, 0.0, 1));
    assert_eq!(audit.clock, 1.0);
}

#[test]
fn sim_without_table_uses_first_table() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let defs = load(root.path(), "basic").unwrap().sim_defs().unwrap();
    assert_eq!(defs[1].table, "ticket");
}

#[test]
fn include_may_reach_a_sibling_scenario() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "shared/steady.rhai", "sleep(seconds(1));");
    write(
        root.path(),
        "chaos/x/scenario.yaml",
        "tables: { row: { count: 0 } }\nsims: { s: { script: !include ../../shared/steady.rhai } }\n",
    );
    let s = load(root.path(), "chaos/x").unwrap();
    assert_eq!(s.sims["s"].script, "sleep(seconds(1));");
}

#[test]
fn include_outside_root_is_refused() {
    let outer = tempfile::tempdir().unwrap();
    write(outer.path(), "secret.rhai", "x");
    let root = outer.path().join("scenarios");
    write(&root, "a/scenario.yaml", "sims: { s: { script: !include ../../secret.rhai } }\n");
    let err = load(&root, "a").unwrap_err();
    assert!(err.contains("outside"), "{err}");
}

#[test]
fn missing_include_names_the_file() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a/scenario.yaml", "sims: { s: { script: !include nope.rhai } }\n");
    let err = load(root.path(), "a").unwrap_err();
    assert!(err.contains("nope.rhai"), "{err}");
}

#[test]
fn unknown_key_names_the_scenario() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "a/scenario.yaml", "sims: {}\nbogus: 1\n");
    let err = load(root.path(), "a").unwrap_err();
    assert!(err.contains("a/scenario.yaml") && err.contains("bogus"), "{err}");
}

#[test]
fn durations_parse_like_vantage_ui() {
    assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
    assert_eq!(parse_duration("1.5s").unwrap(), Duration::from_millis(1500));
    assert_eq!(parse_duration("2m").unwrap(), Duration::from_secs(120));
    assert_eq!(parse_duration("12h").unwrap(), Duration::from_secs(43_200));
    assert_eq!(parse_duration("3d").unwrap(), Duration::from_secs(259_200));
    assert_eq!(parse_duration("30").unwrap(), Duration::from_secs(30));
    assert!(parse_duration("soon").is_err());
}

#[test]
fn scale_multiplies_sims_and_rows() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let s = load(root.path(), "basic").unwrap().scaled(2.0, 3.0);
    let churn = &s.sims["churn"].spawn;
    assert_eq!((churn.burst, churn.max), (Some(6), Some(20)));
    assert_eq!(s.tables["ticket"].count, 60);
}

#[test]
fn scale_keeps_nonzero_at_least_one() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let s = load(root.path(), "basic").unwrap().scaled(0.01, 0.01);
    assert_eq!(s.sims["churn"].spawn.burst, Some(1));
    assert_eq!(s.sims["churn"].spawn.max, Some(1));
    assert_eq!(s.tables["ticket"].count, 1);
    assert_eq!(s.tables["audit"].count, 0);
}

#[test]
fn without_warm_clears_every_warm() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let s = load(root.path(), "basic").unwrap().without_warm();
    assert!(s.sims.values().all(|d| d.warm.is_none()));
}

#[test]
fn faker_columns_put_id_first() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "basic/scenario.yaml", BASIC);
    write(root.path(), "basic/churn.rhai", "");
    let cols = load(root.path(), "basic").unwrap().faker_columns("ticket");
    let names: Vec<_> = cols.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "status", "amount", "note"]);
    assert_eq!(cols[2].ty, "int");
    assert!(cols[1].generator.is_some() && cols[3].generator.is_none());
}
```

- [ ] **Step 3: Run the tests and check they fail**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --lib 2>&1 | tail -20`
Expected: compile errors (`load`, `Scenario` and so on don't exist yet).

- [ ] **Step 4: Types and `load`**

Create `vantage-faker-stress/src/scenario.rs`:

```rust
//! Scenario files: tables, sims and stress settings, in the shape of a
//! vantage-ui faker datasource. `SimSpec` and `SpawnSpec` mirror
//! vantage-ui's `crates/inventory/src/faker_sims.rs`.

mod build;
mod include;
#[cfg(test)]
mod tests;

use std::path::Path;

use indexmap::IndexMap;
use serde::Deserialize;
use vantage_faker::ColumnGen;

pub use build::parse_duration;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub tables: IndexMap<String, TableSpec>,
    #[serde(default)]
    pub sims: IndexMap<String, SimSpec>,
    #[serde(default)]
    pub stress: StressSpec,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSpec {
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub columns: IndexMap<String, ColumnSpec>,
}

/// A column as a vantage-ui table declares it: a type and an optional generator.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnSpec {
    #[serde(default, rename = "type")]
    pub ty: Option<String>,
    #[serde(default)]
    pub faker: Option<ColumnGen>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimSpec {
    #[serde(default)]
    pub table: Option<String>,
    pub script: String,
    #[serde(default)]
    pub clock: Option<f64>,
    #[serde(default)]
    pub warm: Option<String>,
    #[serde(default)]
    pub spawn: SpawnSpec,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnSpec {
    #[serde(default)]
    pub burst: Option<usize>,
    #[serde(default)]
    pub rate: Option<f64>,
    #[serde(default)]
    pub max: Option<usize>,
    #[serde(default)]
    pub args: IndexMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StressSpec {
    #[serde(default = "default_duration")]
    pub duration: String,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub ramp: RampSpec,
    #[serde(default)]
    pub expect: Option<Expect>,
    /// Attach a Dio to every table, as `--dio` does.
    #[serde(default)]
    pub dio: bool,
}

impl Default for StressSpec {
    fn default() -> Self {
        Self {
            duration: default_duration(),
            limits: Limits::default(),
            ramp: RampSpec::default(),
            expect: None,
            dio: false,
        }
    }
}

fn default_duration() -> String {
    "30s".into()
}

/// Thresholds a ramp stops at.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub cpu_pct: Option<f64>,
    pub event_lag_ms: Option<f64>,
    pub rss_mb: Option<f64>,
}

/// What one ramp unit is. `base` is the unit count at scale 1 (default: the
/// sum of every def's `max`); `sims: false` ramps table rows only.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RampSpec {
    #[serde(default)]
    pub base: Option<usize>,
    #[serde(default = "yes")]
    pub sims: bool,
}

impl Default for RampSpec {
    fn default() -> Self {
        Self { base: None, sims: true }
    }
}

fn yes() -> bool {
    true
}

/// What a chaos scenario must show to count as contained.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    #[serde(default)]
    pub errored_min: u64,
    #[serde(default)]
    pub max_threads: Option<usize>,
}

/// Load `<root>/<name>/scenario.yaml`, resolving `!include`s relative to
/// its directory but never outside `root`.
pub fn load(root: &Path, name: &str) -> Result<Scenario, String> {
    let file = root.join(name).join("scenario.yaml");
    let label = format!("{name}/scenario.yaml");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("{label}: {e}"))?;
    let raw: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&text).map_err(|e| format!("{label}: {e}"))?;
    let resolved = include::resolve(raw, &root.join(name), root)
        .map_err(|e| format!("{label}: {e}"))?;
    serde_yaml_ng::from_value(resolved).map_err(|e| format!("{label}: {e}"))
}
```

- [ ] **Step 5: `!include`**

Create `vantage-faker-stress/src/scenario/include.rs`:

```rust
//! `!include <path>`: replace the tagged value with the file's text.

use std::path::Path;

use serde_yaml_ng::Value;

/// Resolve every `!include` in `value`. Paths are relative to `dir` and
/// must stay under `root`.
pub(super) fn resolve(value: Value, dir: &Path, root: &Path) -> Result<Value, String> {
    Ok(match value {
        Value::Tagged(tagged) if tagged.tag == "include" => {
            let Value::String(rel) = tagged.value else {
                return Err("!include takes a file path".into());
            };
            Value::String(read(&rel, dir, root)?)
        }
        Value::Tagged(mut tagged) => {
            tagged.value = resolve(tagged.value, dir, root)?;
            Value::Tagged(tagged)
        }
        Value::Mapping(map) => Value::Mapping(
            map.into_iter()
                .map(|(k, v)| Ok((k, resolve(v, dir, root)?)))
                .collect::<Result<_, String>>()?,
        ),
        Value::Sequence(items) => Value::Sequence(
            items
                .into_iter()
                .map(|v| resolve(v, dir, root))
                .collect::<Result<_, String>>()?,
        ),
        other => other,
    })
}

fn read(rel: &str, dir: &Path, root: &Path) -> Result<String, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("scenarios root: {e}"))?;
    let path = dir
        .join(rel)
        .canonicalize()
        .map_err(|e| format!("!include {rel}: {e}"))?;
    if !path.starts_with(&root) {
        return Err(format!("!include {rel}: outside the scenarios directory"));
    }
    std::fs::read_to_string(&path).map_err(|e| format!("!include {rel}: {e}"))
}
```

If `tagged.tag == "include"` doesn't compile against serde_yaml_ng 0.10's `Tag` (it should, because `Tag: PartialEq<str>` ignores the leading `!`), use `tagged.tag.to_string().trim_start_matches('!') == "include"`.

- [ ] **Step 6: Durations, scaling, defs and columns**

Create `vantage-faker-stress/src/scenario/build.rs`:

```rust
//! Turning a loaded scenario into engine inputs: durations, scaling,
//! `SimDef`s with vantage-ui's defaults, and `FakerColumn`s.

use std::time::Duration;

use vantage_faker::{FakerColumn, SimDef};

use super::Scenario;

/// `500ms`, `1.5s`, `2m`, `6h`, `3d` or bare seconds, as vantage-ui reads them.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let s = s.trim();
    let bad = || format!("`{s}` is not a duration (e.g. 500ms, 30s, 5m, 12h, 3d)");
    let (num, unit) = match s {
        _ if s.ends_with("ms") => (&s[..s.len() - 2], 0.001),
        _ if s.ends_with('s') => (&s[..s.len() - 1], 1.0),
        _ if s.ends_with('m') => (&s[..s.len() - 1], 60.0),
        _ if s.ends_with('h') => (&s[..s.len() - 1], 3600.0),
        _ if s.ends_with('d') => (&s[..s.len() - 1], 86_400.0),
        _ => (s, 1.0),
    };
    let n: f64 = num.trim().parse().map_err(|_| bad())?;
    Duration::try_from_secs_f64(n * unit).map_err(|_| bad())
}

/// `n × factor`, rounded; a non-zero `n` stays at least 1.
fn scale(n: usize, factor: f64) -> usize {
    if n == 0 {
        return 0;
    }
    ((n as f64 * factor).round() as usize).max(1)
}

impl Scenario {
    pub fn duration(&self) -> Result<Duration, String> {
        parse_duration(&self.stress.duration)
    }

    /// Multiply every `burst` and `max` by `sims` and every table `count` by `rows`.
    pub fn scaled(&self, sims: f64, rows: f64) -> Scenario {
        let mut s = self.clone();
        for spec in s.sims.values_mut() {
            let spawn = &mut spec.spawn;
            spawn.burst = Some(scale(spawn.burst.unwrap_or(1), sims));
            spawn.max = Some(scale(spawn.max.unwrap_or(1), sims));
        }
        for table in s.tables.values_mut() {
            table.count = scale(table.count, rows);
        }
        s
    }

    pub fn without_warm(&self) -> Scenario {
        let mut s = self.clone();
        for spec in s.sims.values_mut() {
            spec.warm = None;
        }
        s
    }

    /// Sum of every def's resolved `max`.
    pub fn max_total(&self) -> usize {
        self.sims.values().map(|s| s.spawn.max.unwrap_or(1)).sum()
    }

    /// One `SimDef` per sim, with vantage-ui's defaults: `burst: 1`,
    /// `rate: 0`, `max: 1`, `clock: 1`, and the first table when no `table:`.
    pub fn sim_defs(&self) -> Result<Vec<SimDef>, String> {
        let first = self.tables.keys().next().cloned();
        self.sims
            .iter()
            .map(|(name, spec)| {
                let table = spec
                    .table
                    .clone()
                    .or_else(|| first.clone())
                    .ok_or_else(|| format!("sim {name}: no table to write"))?;
                let spawn = &spec.spawn;
                let mut def = SimDef::new(name, table, spec.script.clone())
                    .with_spawn(
                        spawn.burst.unwrap_or(1),
                        spawn.rate.unwrap_or(0.0),
                        spawn.max.unwrap_or(1),
                    )
                    .with_clock(spec.clock.unwrap_or(1.0));
                if let Some(warm) = &spec.warm {
                    def = def.with_warm(parse_duration(warm).map_err(|e| format!("sim {name}: warm: {e}"))?);
                }
                if !spawn.args.is_empty() {
                    let args = spawn.args.iter().map(|(k, v)| (k.clone(), v.clone()));
                    def = def.with_args(serde_json::Value::Object(args.collect()));
                }
                Ok(def)
            })
            .collect()
    }

    /// `id` first (unless declared), then the declared columns in order.
    pub fn faker_columns(&self, table: &str) -> Vec<FakerColumn> {
        let Some(spec) = self.tables.get(table) else {
            return Vec::new();
        };
        let mut cols = Vec::new();
        if !spec.columns.contains_key("id") {
            let mut id = FakerColumn::new("id", "string");
            id.flags.push("id".into());
            cols.push(id);
        }
        for (name, c) in &spec.columns {
            let mut col = FakerColumn::new(name, c.ty.as_deref().unwrap_or("string"));
            col.generator = c.faker.clone();
            cols.push(col);
        }
        cols
    }
}
```

- [ ] **Step 7: Run the tests and check they pass**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --lib 2>&1 | tail -20`
Expected: all 12 tests pass.

- [ ] **Step 8: Commit**

```bash
cd /Users/rw/Work/vantage-faker-rework
git add Cargo.toml vantage-faker-stress/Cargo.toml vantage-faker-stress/Cargo.lock vantage-faker-stress/src
git commit -m "vantage-faker-stress: crate scaffold and scenario loading"
```

---

### Task 3: Process sampling (threads, CPU, RSS) and the panic counter

**Files:**
- Create: `vantage-faker-stress/src/threads.rs`
- Create: `vantage-faker-stress/src/panics.rs`
- Create: `vantage-faker-stress/src/sampler.rs`
- Modify: `vantage-faker-stress/src/lib.rs` (add `pub mod panics; pub mod sampler; pub mod threads;`)

**Interfaces:**
- Consumes: `vantage_faker::SimStats` (Task 1)
- Produces:
  - `threads::process_threads() -> usize`
  - `panics::install()`, which chains a counting hook once
  - `panics::count() -> u64`
  - `sampler::Sample` (`Clone, Debug, Serialize, Deserialize`), with fields `t: f64, live: usize, spawned: u64, ended: u64, errored: u64, threads: usize, cpu_pct: f64, rss_mb: f64, writes_per_s: f64, events_per_s: f64, lagged: u64, lag_ms: f64`
  - `sampler::EventTotals { delivered: u64, lagged: u64, backlog: usize }`
  - `sampler::Sampler::new() -> Sampler`
  - `Sampler::sample(&mut self, stats: SimStats, events: EventTotals) -> Sample`
  - `Sampler::cpu_now(&mut self) -> f64`

- [ ] **Step 1: Write the failing tests**

Put these at the bottom of `vantage-faker-stress/src/sampler.rs`, in a `#[cfg(test)] mod tests` block, once the file exists in Step 3. Create the file with only the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use vantage_faker::SimStats;

    #[test]
    fn thread_count_sees_this_process() {
        assert!(crate::threads::process_threads() >= 1);
    }

    #[test]
    fn rates_are_deltas_per_second() {
        let mut s = Sampler::new();
        let first = s.sample(
            SimStats { writes: 10, ..Default::default() },
            EventTotals { delivered: 5, lagged: 0, backlog: 0 },
        );
        assert_eq!(first.writes_per_s, 0.0, "first sample has no previous one");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let second = s.sample(
            SimStats { writes: 60, ..Default::default() },
            EventTotals { delivered: 55, lagged: 2, backlog: 10 },
        );
        assert!((80.0..=120.0).contains(&second.writes_per_s), "{}", second.writes_per_s);
        assert!((80.0..=120.0).contains(&second.events_per_s));
        assert_eq!(second.lagged, 2);
        assert!(second.lag_ms > 0.0);
        assert!(second.rss_mb > 0.0);
    }

    #[test]
    fn panic_counter_counts() {
        crate::panics::install();
        let before = crate::panics::count();
        let _ = std::thread::spawn(|| panic!("counted")).join();
        assert_eq!(crate::panics::count(), before + 1);
    }
}
```

- [ ] **Step 2: Run the tests and check they fail**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --lib sampler 2>&1 | tail -20`
Expected: compile errors for the missing `Sampler`, `threads` and `panics`.

- [ ] **Step 3: Implement**

Create `vantage-faker-stress/src/threads.rs`:

```rust
//! OS threads in this process. sysinfo reports task counts only on Linux,
//! so macOS asks the kernel directly.

#[cfg(target_os = "macos")]
pub fn process_threads() -> usize {
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `info` is a properly sized, writable proc_taskinfo.
    let n = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut info as *mut libc::proc_taskinfo).cast(),
            size,
        )
    };
    if n == size { info.pti_threadnum.max(0) as usize } else { 0 }
}

#[cfg(target_os = "linux")]
pub fn process_threads() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("Threads:"))
                .and_then(|n| n.trim().parse().ok())
        })
        .unwrap_or(0)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn process_threads() -> usize {
    0
}
```

Create `vantage-faker-stress/src/panics.rs`:

```rust
//! Counts panics anywhere in the process, so a chaos run can tell a sim
//! that errored from one that took a thread down.

use std::sync::Once;
use std::sync::atomic::{AtomicU64, Ordering};

static PANICS: AtomicU64 = AtomicU64::new(0);
static INSTALL: Once = Once::new();

/// Chain a counting hook in front of the current panic hook. Idempotent.
pub fn install() {
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            PANICS.fetch_add(1, Ordering::Relaxed);
            previous(info);
        }));
    });
}

pub fn count() -> u64 {
    PANICS.load(Ordering::Relaxed)
}
```

Write `vantage-faker-stress/src/sampler.rs`, above the tests block:

```rust
//! One row of measurements a second: engine counters, process cost and
//! event flow.

use std::time::Instant;

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use vantage_faker::SimStats;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    /// Seconds since the sampler was created.
    pub t: f64,
    pub live: usize,
    pub spawned: u64,
    pub ended: u64,
    pub errored: u64,
    pub threads: usize,
    /// Percent of one core; 400 means four cores busy.
    pub cpu_pct: f64,
    pub rss_mb: f64,
    pub writes_per_s: f64,
    pub events_per_s: f64,
    /// Events dropped so far because a subscriber fell behind.
    pub lagged: u64,
    /// Estimated time for subscribers to drain their backlog.
    pub lag_ms: f64,
}

/// Event totals summed over every table's subscriber.
#[derive(Clone, Copy, Debug, Default)]
pub struct EventTotals {
    pub delivered: u64,
    pub lagged: u64,
    /// Events queued in the broadcast channels, not yet received.
    pub backlog: usize,
}

pub struct Sampler {
    sys: System,
    pid: Pid,
    started: Instant,
    last: Option<(Instant, u64, u64)>,
}

impl Sampler {
    pub fn new() -> Self {
        let mut s = Self {
            sys: System::new(),
            pid: sysinfo::get_current_pid().expect("own pid"),
            started: Instant::now(),
            last: None,
        };
        s.refresh();
        s
    }

    fn refresh(&mut self) {
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
    }

    /// CPU percent since the previous refresh.
    pub fn cpu_now(&mut self) -> f64 {
        self.refresh();
        self.sys.process(self.pid).map_or(0.0, |p| f64::from(p.cpu_usage()))
    }

    pub fn sample(&mut self, stats: SimStats, events: EventTotals) -> Sample {
        let cpu_pct = self.cpu_now();
        let rss_mb = self
            .sys
            .process(self.pid)
            .map_or(0.0, |p| p.memory() as f64 / (1024.0 * 1024.0));
        let now = Instant::now();
        let (writes_per_s, events_per_s) = match self.last {
            Some((at, writes, delivered)) => {
                let dt = now.duration_since(at).as_secs_f64().max(1e-3);
                (
                    stats.writes.saturating_sub(writes) as f64 / dt,
                    events.delivered.saturating_sub(delivered) as f64 / dt,
                )
            }
            None => (0.0, 0.0),
        };
        self.last = Some((now, stats.writes, events.delivered));
        let lag_ms = events.backlog as f64 / events_per_s.max(1.0) * 1000.0;
        Sample {
            t: now.duration_since(self.started).as_secs_f64(),
            live: stats.live,
            spawned: stats.spawned,
            ended: stats.ended,
            errored: stats.errored,
            threads: crate::threads::process_threads(),
            cpu_pct,
            rss_mb,
            writes_per_s,
            events_per_s,
            lagged: events.lagged,
            lag_ms,
        }
    }
}

impl Default for Sampler {
    fn default() -> Self {
        Self::new()
    }
}
```

sysinfo's API has changed across 0.3x releases. If `ProcessRefreshKind::nothing()` or the three-argument `refresh_processes_specifics` doesn't match 0.39, check `cargo doc -p sysinfo --open` and adjust to what the installed version exposes. The behaviour needed is: refresh CPU and memory for our own pid only.

Add the modules to `vantage-faker-stress/src/lib.rs`:

```rust
pub mod panics;
pub mod sampler;
pub mod scenario;
pub mod threads;
```

- [ ] **Step 4: Run the tests and check they pass**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --lib 2>&1 | tail -20`
Expected: all tests pass (12 from Task 2 plus 3 new).

- [ ] **Step 5: Commit**

```bash
cd /Users/rw/Work/vantage-faker-rework
git add vantage-faker-stress
git commit -m "vantage-faker-stress: per-second sampler for threads, cpu, rss and event rates"
```

---

### Task 4: Event load: counting subscribers and the optional Dio

**Files:**
- Create: `vantage-faker-stress/src/load.rs`
- Modify: `vantage-faker-stress/src/lib.rs` (add `pub mod load;`)

**Interfaces:**
- Consumes: `vantage_faker::{FakerHandle}`, `vantage_vista::Vista`, `vantage_diorama::{Lens, ChangeEvent, SortDir, TableScenery}`, and `sampler::EventTotals` (Task 3)
- Produces:
  - `load::TableLoad`, with `TableLoad::totals(&self) -> EventTotals`
  - `load::attach(vista: Vista, handle: &FakerHandle, lens: Option<&Arc<Lens>>) -> Result<TableLoad, String>`, which is `async`
  - `load::lens(cache_dir: &Path) -> Result<Arc<Lens>, String>`
  - `load::sum(loads: &[TableLoad]) -> EventTotals`

- [ ] **Step 1: Write the failing test**

At the bottom of `vantage-faker-stress/src/load.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use vantage_faker::{FakerColumn, FakerTable, StaticEffect};

    fn table() -> FakerTable {
        let mut id = FakerColumn::new("id", "string");
        id.flags.push("id".into());
        FakerTable::build(
            "t",
            vec![id, FakerColumn::new("note", "string")],
            "id",
            Box::new(StaticEffect { count: 5 }),
        )
    }

    async fn push_three(handle: &vantage_faker::FakerHandle) {
        for _ in 0..3 {
            handle.ctx().push();
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn counting_subscriber_sees_every_event() {
        let (vista, handle) = table().split();
        let load = attach(vista, &handle, None).await.unwrap();
        push_three(&handle).await;
        let t = load.totals();
        assert_eq!(t.delivered, 3);
        assert_eq!(t.backlog, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dio_subscriber_applies_events() {
        let dir = tempfile::tempdir().unwrap();
        let lens = lens(dir.path()).unwrap();
        let (vista, handle) = table().split();
        let load = attach(vista, &handle, Some(&lens)).await.unwrap();
        push_three(&handle).await;
        assert_eq!(load.totals().delivered, 3);
        assert_eq!(load.scenery_rows(), Some(8));
    }
}
```

- [ ] **Step 2: Run the tests and check they fail**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --lib load 2>&1 | tail -20`
Expected: compile errors (`attach` and `lens` are missing).

- [ ] **Step 3: Implement**

Write `vantage-faker-stress/src/load.rs`, above the tests:

```rust
//! Consumers for each table's events. Without a Dio a subscriber only
//! counts; with one it applies every event to a Dio and keeps a sorted
//! TableScenery open, which is the work a vantage-ui grid does.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::broadcast::{self, error::RecvError};
use tokio::task::JoinHandle;
use vantage_diorama::{ChangeEvent, Lens, SortDir, TableScenery};
use vantage_faker::FakerHandle;
use vantage_vista::Vista;

use crate::sampler::EventTotals;

/// Rows a scenery keeps materialized, like a grid's visible page.
const VIEWPORT: usize = 50;

pub struct TableLoad {
    delivered: Arc<AtomicU64>,
    lagged: Arc<AtomicU64>,
    events: broadcast::Sender<ChangeEvent>,
    scenery: Option<Arc<dyn TableScenery>>,
    task: JoinHandle<()>,
}

impl TableLoad {
    pub fn totals(&self) -> EventTotals {
        EventTotals {
            delivered: self.delivered.load(Ordering::Relaxed),
            lagged: self.lagged.load(Ordering::Relaxed),
            backlog: self.events.len(),
        }
    }

    /// Rows the scenery holds, when a Dio is attached.
    pub fn scenery_rows(&self) -> Option<usize> {
        self.scenery.as_ref().map(|s| s.row_count())
    }
}

impl Drop for TableLoad {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn sum(loads: &[TableLoad]) -> EventTotals {
    loads.iter().map(TableLoad::totals).fold(EventTotals::default(), |a, b| EventTotals {
        delivered: a.delivered + b.delivered,
        lagged: a.lagged + b.lagged,
        backlog: a.backlog + b.backlog,
    })
}

/// A lens whose Dios apply faker events in place, as vantage-ui's do.
pub fn lens(cache_dir: &Path) -> Result<Arc<Lens>, String> {
    let lens = Lens::new()
        .cache_at(cache_dir.join("cache.redb"))
        .on_event(|dio, evt| {
            let dio = dio.clone();
            async move {
                match evt {
                    ChangeEvent::Inserted { id, new: Some(record) }
                    | ChangeEvent::Updated { id, new: Some(record) } => {
                        dio.patched(id, record).await?
                    }
                    ChangeEvent::Deleted { id } => dio.removed(id).await?,
                    ChangeEvent::Invalidated => {
                        dio.cache().clear().await?;
                        dio.notify_dataset_changed();
                    }
                    _ => {}
                }
                Ok(())
            }
        })
        .build()
        .map_err(|e| format!("lens: {e}"))?;
    Ok(Arc::new(lens))
}

/// Subscribe to `handle`'s events. With a `lens`, `vista` becomes a Dio and
/// every event is applied to it.
pub async fn attach(
    vista: Vista,
    handle: &FakerHandle,
    lens: Option<&Arc<Lens>>,
) -> Result<TableLoad, String> {
    let mut rx = handle.events.subscribe();
    let delivered = Arc::new(AtomicU64::new(0));
    let lagged = Arc::new(AtomicU64::new(0));
    let (dio, scenery) = match lens {
        Some(lens) => {
            let dio = lens.make_dio(vista).await.map_err(|e| format!("dio: {e}"))?;
            let scenery: Arc<dyn TableScenery> = dio
                .table_scenery()
                .sort("id", SortDir::Asc)
                .open()
                .await
                .map_err(|e| format!("scenery: {e}"))?;
            scenery.set_viewport(0..VIEWPORT);
            (Some(dio), Some(scenery))
        }
        None => (None, None),
    };
    let (d, l) = (delivered.clone(), lagged.clone());
    let task = tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(evt) => {
                    if let Some(dio) = &dio {
                        let _ = dio.handle_event(evt).await;
                    }
                    d.fetch_add(1, Ordering::Relaxed);
                }
                Err(RecvError::Lagged(n)) => {
                    l.fetch_add(n, Ordering::Relaxed);
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
    Ok(TableLoad {
        delivered,
        lagged,
        events: handle.events.clone(),
        scenery,
        task,
    })
}
```

Notes for the implementer:
- `dio.handle_event` errors are ignored on purpose. The harness measures throughput, and a failed apply shows up as the scenery's row count drifting. Don't log per event, because at thousands of events a second that would dominate the measurement.
- `Sender::len()` counts messages held for the slowest receiver. That is the backlog we want.
- If `row_count()` settles after a short delay (the scenery re-materializes asynchronously), the test's 300 ms sleep covers it. If it still flakes, poll up to 2 s until it reads 8.

- [ ] **Step 4: Run the tests and check they pass**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --lib load 2>&1 | tail -20`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
cd /Users/rw/Work/vantage-faker-rework
git add vantage-faker-stress/src
git commit -m "vantage-faker-stress: event subscribers, optional Dio and scenery load"
```

---

### Task 5: Runner: one pass of a scenario

**Files:**
- Create: `vantage-faker-stress/src/runner.rs`
- Modify: `vantage-faker-stress/src/lib.rs` (add `pub mod runner;`)
- Test: `vantage-faker-stress/tests/runner.rs`

**Interfaces:**
- Consumes: `scenario::Scenario` (Task 2), `sampler::{Sampler, Sample}` (Task 3), `load::{attach, lens, sum, TableLoad}` (Task 4), `panics` (Task 3), `SimEngine::stats` (Task 1)
- Produces:
  - `runner::RunOpts { duration: Duration, dio: bool }`
  - `runner::RunOutput { samples: Vec<Sample>, warm_secs: Option<f64>, baseline_cpu: f64, panics: u64 }`
  - `runner::run(scenario: &Scenario, opts: &RunOpts, on_sample: impl FnMut(&Sample)) -> Result<RunOutput, String>`, which is `async`

- [ ] **Step 1: Write the failing test**

Create `vantage-faker-stress/tests/runner.rs`:

```rust
use std::fs;
use std::time::Duration;

use vantage_faker_stress::runner::{RunOpts, run};
use vantage_faker_stress::scenario::load;

const SCENARIO: &str = r#"
seed: 3
tables:
  row: { count: 10, columns: { note: {} } }
sims:
  tick:
    script: "loop { let id = insert(#{ note: \"x\" }); sleep(seconds(0.2)); delete(id); }"
    spawn: { burst: 5, max: 5 }
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_short_run_samples_live_sims_and_events() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("t")).unwrap();
    fs::write(root.path().join("t/scenario.yaml"), SCENARIO).unwrap();
    let scenario = load(root.path(), "t").unwrap();
    let mut seen = 0;
    let out = run(
        &scenario,
        &RunOpts { duration: Duration::from_secs(3), dio: true },
        |_| seen += 1,
    )
    .await
    .unwrap();
    assert!(out.samples.len() >= 2, "{} samples", out.samples.len());
    assert_eq!(seen, out.samples.len());
    let last = out.samples.last().unwrap();
    assert_eq!(last.live, 5);
    assert!(last.spawned >= 5);
    assert!(out.samples.iter().any(|s| s.events_per_s > 0.0));
    assert!(out.warm_secs.is_none());
    assert_eq!(out.panics, 0);
}
```

- [ ] **Step 2: Run the test and check it fails**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --test runner 2>&1 | tail -20`
Expected: a compile error (`runner` missing).

- [ ] **Step 3: Implement**

Create `vantage-faker-stress/src/runner.rs`:

```rust
//! One pass of a scenario: build its tables, attach consumers, take an
//! idle CPU baseline, start the engine (timing any warm start), sample once
//! a second for the run's duration, then stop.

use std::time::{Duration, Instant};

use vantage_faker::{FakerHandle, FakerTable, SimEngine, StaticEffect};

use crate::load::{self, TableLoad};
use crate::sampler::{Sample, Sampler};
use crate::scenario::Scenario;
use crate::panics;

const TICK: Duration = Duration::from_secs(1);

pub struct RunOpts {
    pub duration: Duration,
    pub dio: bool,
}

pub struct RunOutput {
    pub samples: Vec<Sample>,
    /// Seconds `start()` spent in the warm start; `None` when no def warms.
    pub warm_secs: Option<f64>,
    /// CPU percent over the idle second before the engine started.
    pub baseline_cpu: f64,
    /// Panics anywhere in the process during the run.
    pub panics: u64,
}

pub async fn run(
    scenario: &Scenario,
    opts: &RunOpts,
    mut on_sample: impl FnMut(&Sample),
) -> Result<RunOutput, String> {
    panics::install();
    let panics_before = panics::count();
    let defs = scenario.sim_defs()?;
    let warms = defs.iter().any(|d| d.warm.is_some());

    let cache = tempfile::tempdir().map_err(|e| format!("cache dir: {e}"))?;
    let lens = if opts.dio || scenario.stress.dio {
        Some(load::lens(cache.path())?)
    } else {
        None
    };

    let mut handles: Vec<(String, FakerHandle)> = Vec::new();
    let mut loads: Vec<TableLoad> = Vec::new();
    for (name, spec) in &scenario.tables {
        let table = FakerTable::build(
            name.clone(),
            scenario.faker_columns(name),
            "id",
            Box::new(StaticEffect { count: spec.count }),
        );
        let (vista, handle) = table.split();
        loads.push(load::attach(vista, &handle, lens.as_ref()).await?);
        handles.push((name.clone(), handle));
    }

    let mut sampler = Sampler::new();
    tokio::time::sleep(TICK).await;
    let baseline_cpu = sampler.cpu_now();

    let mut builder = SimEngine::builder();
    for (name, handle) in &handles {
        builder = builder.table(name.clone(), handle.ctx());
    }
    for def in defs {
        builder = builder.sim(def);
    }
    if let Some(seed) = scenario.seed {
        builder = builder.seed(seed);
    }
    let started = Instant::now();
    let engine = tokio::task::spawn_blocking(move || builder.start())
        .await
        .map_err(|e| format!("engine start panicked: {e}"))??;
    let warm_secs = warms.then(|| started.elapsed().as_secs_f64());

    let mut samples = Vec::new();
    let live_from = Instant::now();
    let mut ticker = tokio::time::interval(TICK);
    ticker.tick().await;
    while live_from.elapsed() < opts.duration {
        ticker.tick().await;
        let sample = sampler.sample(engine.stats(), load::sum(&loads));
        on_sample(&sample);
        samples.push(sample);
    }

    tokio::task::spawn_blocking(move || engine.stop())
        .await
        .map_err(|e| format!("engine stop panicked: {e}"))?;
    drop(loads);
    drop(handles);
    Ok(RunOutput {
        samples,
        warm_secs,
        baseline_cpu,
        panics: panics::count() - panics_before,
    })
}
```

Note: `engine.stop()` joins every sim thread, so it runs on the blocking pool. `stop` consumes nothing, so moving `engine` into the closure drops it there as well. `Drop` calls `stop` again, and `stop` is idempotent.

Add `pub mod runner;` and `pub mod load;` to `lib.rs` in alphabetical order, if they aren't there yet.

- [ ] **Step 4: Run the test and check it passes**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --test runner 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/rw/Work/vantage-faker-rework
git add vantage-faker-stress
git commit -m "vantage-faker-stress: runner builds tables and engine, samples a timed pass"
```

---

### Task 6: Reports, verdicts, ramp and the CLI

**Files:**
- Create: `vantage-faker-stress/src/report.rs` (summary maths, the `Report` JSON, printing)
- Create: `vantage-faker-stress/src/report/compare.rs`
- Create: `vantage-faker-stress/src/verdict.rs`
- Create: `vantage-faker-stress/src/ramp.rs`
- Replace: `vantage-faker-stress/src/main.rs`
- Modify: `vantage-faker-stress/src/lib.rs` (add `pub mod ramp; pub mod report; pub mod verdict;`)

**Interfaces:**
- Consumes: `runner::{run, RunOpts, RunOutput}` (Task 5), `sampler::Sample` (Task 3), `scenario::{load, Scenario, Expect, Limits}` (Task 2)
- Produces:
  - `report::Summary`, with fields `peak_*`/`mean_*` for `cpu_pct, rss_mb, threads, live, writes_per_s, events_per_s, lag_ms`, plus `lagged: u64, spawned: u64, ended: u64, errored: u64, cpu_per_sim: f64, rss_kb_per_sim: f64`
  - `report::summarize(samples: &[Sample]) -> Summary`
  - `report::Step { target: Option<usize>, summary: Summary, samples: Vec<Sample>, warm_secs: Option<f64>, verdict: Option<String>, stop_reason: Option<String> }`
  - `report::Report { scenario: String, mode: String, faker_version: String, git_rev: String, profile: String, steps: Vec<Step> }`
  - `report::print_header()`, `report::print_row(&Sample)`, `report::print_summary(&Step)`
  - `report::compare::compare(a: &Report, b: &Report) -> String`
  - `verdict::verdict(expect: &Expect, out: &RunOutput) -> Result<(), Vec<String>>`, where `Ok` means contained
  - `ramp::factor(scenario: &Scenario, target: usize) -> (f64, f64)`, the `(sims, rows)` scale
  - `ramp::breach(limits: &Limits, s: &Summary) -> Option<String>`

- [ ] **Step 1: Write the failing tests**

Create `vantage-faker-stress/src/report/tests.rs`, and register `#[cfg(test)] mod tests;` in `report.rs`:

```rust
use super::*;
use crate::sampler::Sample;

fn sample(t: f64, live: usize, cpu: f64, rss: f64) -> Sample {
    Sample {
        t, live, spawned: live as u64, ended: 0, errored: 0, threads: live + 10,
        cpu_pct: cpu, rss_mb: rss, writes_per_s: 100.0, events_per_s: 90.0,
        lagged: 0, lag_ms: 5.0,
    }
}

#[test]
fn summary_takes_peaks_means_and_tail_cost_per_sim() {
    let s = summarize(&[
        sample(1.0, 100, 50.0, 100.0),
        sample(2.0, 100, 150.0, 200.0),
        sample(3.0, 200, 100.0, 300.0),
        sample(4.0, 200, 100.0, 300.0),
    ]);
    assert_eq!(s.peak_cpu_pct, 150.0);
    assert_eq!(s.mean_cpu_pct, 100.0);
    assert_eq!(s.peak_live, 200.0);
    // Tail half: samples 3 and 4, 100 % over 200 sims, 300 MB over 200 sims.
    assert!((s.cpu_per_sim - 0.5).abs() < 1e-9);
    assert!((s.rss_kb_per_sim - 1536.0).abs() < 1e-9);
}

#[test]
fn summary_of_nothing_is_zero() {
    let s = summarize(&[]);
    assert_eq!(s.peak_cpu_pct, 0.0);
    assert_eq!(s.cpu_per_sim, 0.0);
}

#[test]
fn compare_shows_both_sides_and_delta() {
    let step = |cpu| Step {
        target: None,
        summary: summarize(&[sample(1.0, 10, cpu, 50.0)]),
        samples: vec![],
        warm_secs: None,
        verdict: None,
        stop_reason: None,
    };
    let a = Report { scenario: "churn".into(), mode: "run".into(), faker_version: "0.7.0".into(),
        git_rev: "aaa".into(), profile: "debug".into(), steps: vec![step(100.0)] };
    let mut b = a.clone();
    b.faker_version = "0.8.0".into();
    b.steps = vec![step(50.0)];
    let text = compare::compare(&a, &b);
    assert!(text.contains("0.7.0") && text.contains("0.8.0"));
    assert!(text.contains("-50.0%"), "{text}");
}
```

Create `vantage-faker-stress/src/verdict.rs`, with tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::RunOutput;
    use crate::sampler::Sample;
    use crate::scenario::Expect;

    fn out(cpus: &[f64], errored: u64, threads: usize, panics: u64) -> RunOutput {
        RunOutput {
            samples: cpus.iter().enumerate().map(|(i, &c)| Sample {
                t: i as f64, live: 1, spawned: 1, ended: 0, errored, threads,
                cpu_pct: c, rss_mb: 10.0, writes_per_s: 0.0, events_per_s: 0.0,
                lagged: 0, lag_ms: 0.0,
            }).collect(),
            warm_secs: None,
            baseline_cpu: 2.0,
            panics,
        }
    }

    #[test]
    fn contained_when_cpu_settles_and_errors_seen() {
        let e = Expect { errored_min: 1, max_threads: Some(50) };
        assert!(verdict(&e, &out(&[100.0, 100.0, 5.0, 4.0, 3.0], 1, 40, 0)).is_ok());
    }

    #[test]
    fn pinned_cpu_is_not_contained() {
        let e = Expect { errored_min: 0, max_threads: None };
        let why = verdict(&e, &out(&[100.0, 100.0, 100.0], 0, 10, 0)).unwrap_err();
        assert!(why[0].contains("cpu"), "{why:?}");
    }

    #[test]
    fn missing_errors_threads_and_panics_are_each_reported() {
        let e = Expect { errored_min: 2, max_threads: Some(20) };
        let why = verdict(&e, &out(&[1.0, 1.0, 1.0], 1, 30, 1)).unwrap_err();
        assert_eq!(why.len(), 3, "{why:?}");
    }
}
```

Create `vantage-faker-stress/src/ramp.rs`, with tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::summarize;
    use crate::scenario::{Limits, load};

    fn scenario(yaml: &str) -> Scenario {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("s")).unwrap();
        std::fs::write(root.path().join("s/scenario.yaml"), yaml).unwrap();
        load(root.path(), "s").unwrap()
    }

    #[test]
    fn factor_defaults_to_sum_of_max() {
        let s = scenario("tables: { t: { count: 10 } }\nsims:\n  a: { script: x, spawn: { max: 30 } }\n  b: { script: x, spawn: { max: 70 } }\n");
        assert_eq!(factor(&s, 200), (2.0, 2.0));
    }

    #[test]
    fn factor_can_ramp_rows_only() {
        let s = scenario("tables: { t: { count: 1000 } }\nsims: { a: { script: x } }\nstress: { ramp: { base: 1000, sims: false } }\n");
        assert_eq!(factor(&s, 500), (1.0, 0.5));
    }

    #[test]
    fn ramp_stops_before_max_live() {
        let s = scenario("tables: { t: { count: 1 } }\nsims: { a: { script: x, spawn: { burst: 10, max: 10 } } }\n");
        let (sims, rows) = factor(&s, 2000);
        assert!(s.scaled(sims, rows).max_total() > vantage_faker::sim::MAX_LIVE);
        assert!(over_max_live(&s, 2000).is_some());
        assert!(over_max_live(&s, 500).is_none());
    }

    #[test]
    fn breach_names_the_limit() {
        let limits = Limits { cpu_pct: Some(50.0), event_lag_ms: None, rss_mb: None };
        let summary = summarize(&[]);
        assert!(breach(&limits, &summary).is_none());
        let mut hot = summary.clone();
        hot.mean_cpu_pct = 80.0;
        assert!(breach(&limits, &hot).unwrap().contains("cpu"));
    }
}
```

- [ ] **Step 2: Run the tests and check they fail**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --lib 2>&1 | tail -20`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement `report.rs`**

```rust
//! Summaries, the JSON report, and the live table the CLI prints.

pub mod compare;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

use crate::sampler::Sample;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Summary {
    pub peak_cpu_pct: f64,
    pub mean_cpu_pct: f64,
    pub peak_rss_mb: f64,
    pub mean_rss_mb: f64,
    pub peak_threads: f64,
    pub mean_threads: f64,
    pub peak_live: f64,
    pub mean_live: f64,
    pub peak_writes_per_s: f64,
    pub mean_writes_per_s: f64,
    pub peak_events_per_s: f64,
    pub mean_events_per_s: f64,
    pub peak_lag_ms: f64,
    pub mean_lag_ms: f64,
    pub lagged: u64,
    pub spawned: u64,
    pub ended: u64,
    pub errored: u64,
    /// CPU percent per live sim, over the second half of the run.
    pub cpu_per_sim: f64,
    /// Resident KB per live sim, over the second half of the run.
    pub rss_kb_per_sim: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Step {
    /// Ramp target in live sims (or rows); `None` for a plain run.
    pub target: Option<usize>,
    pub summary: Summary,
    pub samples: Vec<Sample>,
    pub warm_secs: Option<f64>,
    pub verdict: Option<String>,
    pub stop_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub scenario: String,
    pub mode: String,
    pub faker_version: String,
    pub git_rev: String,
    pub profile: String,
    pub steps: Vec<Step>,
}

fn peak_mean(samples: &[Sample], f: impl Fn(&Sample) -> f64) -> (f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0);
    }
    let peak = samples.iter().map(&f).fold(f64::MIN, f64::max);
    let mean = samples.iter().map(&f).sum::<f64>() / samples.len() as f64;
    (peak, mean)
}

pub fn summarize(samples: &[Sample]) -> Summary {
    let (peak_cpu_pct, mean_cpu_pct) = peak_mean(samples, |s| s.cpu_pct);
    let (peak_rss_mb, mean_rss_mb) = peak_mean(samples, |s| s.rss_mb);
    let (peak_threads, mean_threads) = peak_mean(samples, |s| s.threads as f64);
    let (peak_live, mean_live) = peak_mean(samples, |s| s.live as f64);
    let (peak_writes_per_s, mean_writes_per_s) = peak_mean(samples, |s| s.writes_per_s);
    let (peak_events_per_s, mean_events_per_s) = peak_mean(samples, |s| s.events_per_s);
    let (peak_lag_ms, mean_lag_ms) = peak_mean(samples, |s| s.lag_ms);
    let tail = &samples[samples.len() / 2..];
    let (_, tail_live) = peak_mean(tail, |s| s.live as f64);
    let (_, tail_cpu) = peak_mean(tail, |s| s.cpu_pct);
    let (_, tail_rss) = peak_mean(tail, |s| s.rss_mb);
    let per_sim = |v: f64| if tail_live > 0.0 { v / tail_live } else { 0.0 };
    let last = samples.last();
    Summary {
        peak_cpu_pct, mean_cpu_pct, peak_rss_mb, mean_rss_mb, peak_threads, mean_threads,
        peak_live, mean_live, peak_writes_per_s, mean_writes_per_s, peak_events_per_s,
        mean_events_per_s, peak_lag_ms, mean_lag_ms,
        lagged: last.map_or(0, |s| s.lagged),
        spawned: last.map_or(0, |s| s.spawned),
        ended: last.map_or(0, |s| s.ended),
        errored: last.map_or(0, |s| s.errored),
        cpu_per_sim: per_sim(tail_cpu),
        rss_kb_per_sim: per_sim(tail_rss * 1024.0),
    }
}

pub fn print_header() {
    println!(
        "{:>5} {:>6} {:>8} {:>7} {:>7} {:>7} {:>6} {:>7} {:>9} {:>9} {:>7} {:>7}",
        "t", "live", "spawned", "ended", "errored", "threads", "cpu%", "rss", "writes/s",
        "events/s", "lagged", "lag ms"
    );
}

pub fn print_row(s: &Sample) {
    println!(
        "{:>5.0} {:>6} {:>8} {:>7} {:>7} {:>7} {:>6.0} {:>7.1} {:>9.0} {:>9.0} {:>7} {:>7.0}",
        s.t, s.live, s.spawned, s.ended, s.errored, s.threads, s.cpu_pct, s.rss_mb,
        s.writes_per_s, s.events_per_s, s.lagged, s.lag_ms
    );
}

pub fn print_summary(step: &Step) {
    let s = &step.summary;
    if let Some(target) = step.target {
        println!("-- step {target}");
    }
    println!(
        "peak cpu {:.0}%  mean cpu {:.0}%  peak rss {:.1} MB  peak threads {:.0}  peak live {:.0}",
        s.peak_cpu_pct, s.mean_cpu_pct, s.peak_rss_mb, s.peak_threads, s.peak_live
    );
    println!(
        "mean writes/s {:.0}  mean events/s {:.0}  peak lag {:.0} ms  lagged {}",
        s.mean_writes_per_s, s.mean_events_per_s, s.peak_lag_ms, s.lagged
    );
    println!(
        "per sim: {:.3}% cpu, {:.0} KB rss   spawned {} ended {} errored {}",
        s.cpu_per_sim, s.rss_kb_per_sim, s.spawned, s.ended, s.errored
    );
    if let Some(w) = step.warm_secs {
        println!("warm start {w:.2} s");
    }
    if let Some(v) = &step.verdict {
        println!("verdict: {v}");
    }
    if let Some(r) = &step.stop_reason {
        println!("ramp stopped: {r}");
    }
}
```

- [ ] **Step 4: Implement `report/compare.rs`**

```rust
//! Two reports side by side, step by step, with the change from a to b.

use std::fmt::Write as _;

use super::{Report, Summary};

const ROWS: &[(&str, fn(&Summary) -> f64)] = &[
    ("mean cpu%", |s| s.mean_cpu_pct),
    ("peak rss MB", |s| s.peak_rss_mb),
    ("peak threads", |s| s.peak_threads),
    ("mean live", |s| s.mean_live),
    ("mean writes/s", |s| s.mean_writes_per_s),
    ("mean events/s", |s| s.mean_events_per_s),
    ("peak lag ms", |s| s.peak_lag_ms),
    ("cpu% per sim", |s| s.cpu_per_sim),
    ("rss KB per sim", |s| s.rss_kb_per_sim),
];

fn delta(a: f64, b: f64) -> String {
    if a == 0.0 {
        return "-".into();
    }
    format!("{:+.1}%", (b - a) / a * 100.0)
}

pub fn compare(a: &Report, b: &Report) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{} ({})", a.scenario, a.mode);
    let _ = writeln!(
        out,
        "{:<16} {:>14} {:>14} {:>9}",
        "",
        format!("{} {}", a.faker_version, a.git_rev),
        format!("{} {}", b.faker_version, b.git_rev),
        "change"
    );
    for (sa, sb) in a.steps.iter().zip(&b.steps) {
        if let Some(t) = sa.target {
            let _ = writeln!(out, "-- step {t}");
        }
        for (label, f) in ROWS {
            let (x, y) = (f(&sa.summary), f(&sb.summary));
            let _ = writeln!(out, "{label:<16} {x:>14.2} {y:>14.2} {:>9}", delta(x, y));
        }
    }
    if a.steps.len() != b.steps.len() {
        let _ = writeln!(out, "(step counts differ: {} vs {})", a.steps.len(), b.steps.len());
    }
    out
}
```

- [ ] **Step 5: Implement `verdict.rs`**

Above its tests:

```rust
//! Chaos verdicts: did the engine contain a misbehaving sim?

use crate::runner::RunOutput;
use crate::scenario::Expect;

/// CPU points above the idle baseline a settled run may still use.
const CPU_SLACK: f64 = 10.0;
/// Samples at the end of the run the CPU check averages.
const TAIL: usize = 3;

/// `Ok` when contained; otherwise every reason it was not.
pub fn verdict(expect: &Expect, out: &RunOutput) -> Result<(), Vec<String>> {
    let mut why = Vec::new();
    let tail = &out.samples[out.samples.len().saturating_sub(TAIL)..];
    if !tail.is_empty() {
        let cpu = tail.iter().map(|s| s.cpu_pct).sum::<f64>() / tail.len() as f64;
        if cpu > out.baseline_cpu + CPU_SLACK {
            why.push(format!(
                "cpu still {cpu:.0}% at the end (baseline {:.0}%)",
                out.baseline_cpu
            ));
        }
    }
    let errored = out.samples.last().map_or(0, |s| s.errored);
    if errored < expect.errored_min {
        why.push(format!("{errored} sims errored, expected at least {}", expect.errored_min));
    }
    if let Some(max) = expect.max_threads {
        let peak = out.samples.iter().map(|s| s.threads).max().unwrap_or(0);
        if peak > max {
            why.push(format!("threads peaked at {peak}, above {max}"));
        }
    }
    if out.panics > 0 {
        why.push(format!("{} panics", out.panics));
    }
    if why.is_empty() { Ok(()) } else { Err(why) }
}

pub fn describe(result: &Result<(), Vec<String>>) -> String {
    match result {
        Ok(()) => "contained".into(),
        Err(why) => format!("NOT contained: {}", why.join("; ")),
    }
}
```

- [ ] **Step 6: Implement `ramp.rs`**

Above its tests:

```rust
//! Ramp steps: how a target maps onto a scenario, and when to stop.

use vantage_faker::sim::MAX_LIVE;

use crate::report::Summary;
use crate::scenario::{Limits, Scenario};

/// `(sims, rows)` scale factors that bring `scenario` to `target` units.
pub fn factor(scenario: &Scenario, target: usize) -> (f64, f64) {
    let base = scenario
        .stress
        .ramp
        .base
        .unwrap_or_else(|| scenario.max_total())
        .max(1);
    let f = target as f64 / base as f64;
    if scenario.stress.ramp.sims { (f, f) } else { (1.0, f) }
}

/// Why `target` can't run: its scaled `max`es pass the engine's cap.
pub fn over_max_live(scenario: &Scenario, target: usize) -> Option<String> {
    let (sims, rows) = factor(scenario, target);
    let total = scenario.scaled(sims, rows).max_total();
    (total > MAX_LIVE).then(|| format!("step {target} needs {total} sims, above MAX_LIVE {MAX_LIVE}"))
}

/// The first limit a step's summary broke.
pub fn breach(limits: &Limits, s: &Summary) -> Option<String> {
    if let Some(max) = limits.cpu_pct
        && s.mean_cpu_pct > max
    {
        return Some(format!("mean cpu {:.0}% above {max}%", s.mean_cpu_pct));
    }
    if let Some(max) = limits.event_lag_ms
        && s.peak_lag_ms > max
    {
        return Some(format!("peak lag {:.0} ms above {max} ms", s.peak_lag_ms));
    }
    if let Some(max) = limits.rss_mb
        && s.peak_rss_mb > max
    {
        return Some(format!("peak rss {:.0} MB above {max} MB", s.peak_rss_mb));
    }
    None
}
```

Check whether `vantage_faker::sim` is a public module. It is (`pub mod sim` in lib.rs under `#[cfg(feature = "rhai")]`).

- [ ] **Step 7: Run the unit tests and check they pass**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --lib 2>&1 | tail -20`
Expected: all pass.

- [ ] **Step 8: Write the CLI**

Replace `vantage-faker-stress/src/main.rs`:

```rust
//! `faker-stress`: run, ramp, list and compare faker sim scenarios.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use vantage_faker_stress::report::{self, Report, Step};
use vantage_faker_stress::runner::{RunOpts, run};
use vantage_faker_stress::scenario::{self, parse_duration};
use vantage_faker_stress::{ramp, verdict};

#[derive(Parser)]
#[command(about = "Stress harness for vantage-faker sims")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// List the scenarios under scenarios/.
    List,
    /// One pass of a scenario.
    Run {
        scenario: String,
        #[arg(long)]
        duration: Option<String>,
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        #[arg(long)]
        dio: bool,
        #[arg(long)]
        json: Option<PathBuf>,
    },
    /// One fresh pass per step, until a limit or MAX_LIVE.
    Ramp {
        scenario: String,
        #[arg(long, value_delimiter = ',', default_values_t = [50, 100, 200, 400, 800])]
        steps: Vec<usize>,
        #[arg(long, default_value = "20s")]
        hold: String,
        #[arg(long)]
        dio: bool,
        #[arg(long)]
        warm: bool,
        #[arg(long)]
        json: Option<PathBuf>,
    },
    /// Two JSON reports side by side.
    Compare { a: PathBuf, b: PathBuf },
}

fn scenarios_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios")
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .init();
    if let Err(e) = dispatch(Cli::parse().cmd).await {
        eprintln!("faker-stress: {e}");
        std::process::exit(1);
    }
}

async fn dispatch(cmd: Cmd) -> Result<(), String> {
    match cmd {
        Cmd::List => {
            for name in list(&scenarios_root()) {
                println!("{name}");
            }
            Ok(())
        }
        Cmd::Run { scenario, duration, scale, dio, json } => {
            let s = scenario::load(&scenarios_root(), &scenario)?.scaled(scale, scale);
            let duration = match duration {
                Some(d) => parse_duration(&d)?,
                None => s.duration()?,
            };
            let step = pass(&s, None, RunOpts { duration, dio }).await?;
            finish(&scenario, "run", vec![step], json)
        }
        Cmd::Ramp { scenario, steps, hold, dio, warm, json } => {
            let base = scenario::load(&scenarios_root(), &scenario)?;
            let base = if warm { base } else { base.without_warm() };
            let hold = parse_duration(&hold)?;
            let mut done = Vec::new();
            for target in steps {
                if let Some(reason) = ramp::over_max_live(&base, target) {
                    println!("ramp stopped: {reason}");
                    if let Some(last) = done.last_mut() {
                        last.stop_reason = Some(reason);
                    }
                    break;
                }
                let (sims, rows) = ramp::factor(&base, target);
                let s = base.scaled(sims, rows);
                let mut step = pass(&s, Some(target), RunOpts { duration: hold, dio }).await?;
                let stop = ramp::breach(&base.stress.limits, &step.summary);
                step.stop_reason = stop.clone();
                done.push(step);
                if stop.is_some() {
                    break;
                }
            }
            finish(&scenario, "ramp", done, json)
        }
        Cmd::Compare { a, b } => {
            let read = |p: &Path| -> Result<Report, String> {
                let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", p.display()))
            };
            print!("{}", report::compare::compare(&read(&a)?, &read(&b)?));
            Ok(())
        }
    }
}

async fn pass(s: &scenario::Scenario, target: Option<usize>, opts: RunOpts) -> Result<Step, String> {
    if let Some(t) = target {
        println!("== step {t}");
    }
    report::print_header();
    let out = run(s, &opts, report::print_row).await?;
    let verdict = s
        .stress
        .expect
        .as_ref()
        .map(|e| verdict::describe(&verdict::verdict(e, &out)));
    let step = Step {
        target,
        summary: report::summarize(&out.samples),
        samples: out.samples,
        warm_secs: out.warm_secs,
        verdict,
        stop_reason: None,
    };
    report::print_summary(&step);
    Ok(step)
}

fn finish(scenario: &str, mode: &str, steps: Vec<Step>, json: Option<PathBuf>) -> Result<(), String> {
    let Some(path) = json else { return Ok(()) };
    let report = Report {
        scenario: scenario.into(),
        mode: mode.into(),
        faker_version: faker_version(),
        git_rev: git_rev(),
        profile: if cfg!(debug_assertions) { "debug" } else { "release" }.into(),
        steps,
    };
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Scenario names: directories under `root` (one level of nesting, for
/// `chaos/*`) that hold a `scenario.yaml`.
fn list(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let dirs = |p: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()
    };
    for dir in dirs(root) {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();
        if dir.join("scenario.yaml").exists() {
            names.push(name);
        } else {
            for sub in dirs(&dir) {
                if sub.join("scenario.yaml").exists() {
                    names.push(format!("{name}/{}", sub.file_name().unwrap().to_string_lossy()));
                }
            }
        }
    }
    names.sort();
    names
}

fn faker_version() -> String {
    include_str!("../../vantage-faker/Cargo.toml")
        .lines()
        .find_map(|l| l.strip_prefix("version = "))
        .map(|v| v.trim_matches('"').to_string())
        .unwrap_or_default()
}

fn git_rev() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}
```

`run` takes `impl FnMut(&Sample)`. Passing `report::print_row` works because it is a `fn(&Sample)`.

- [ ] **Step 9: Build and check `compare` by hand**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo build 2>&1 | tail -5`
Expected: it builds with no warnings.

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo run -- list`
Expected: no output and exit 0, because `scenarios/` doesn't exist yet.

- [ ] **Step 10: Commit**

```bash
cd /Users/rw/Work/vantage-faker-rework
git add vantage-faker-stress/src
git commit -m "vantage-faker-stress: reports, chaos verdicts, ramp and the faker-stress CLI"
```

---

### Task 7: Load scenarios and the smoke test

**Files:**
- Create: `vantage-faker-stress/scenarios/idle/scenario.yaml`, `idle/sleeper.rhai`
- Create: `vantage-faker-stress/scenarios/ticker/scenario.yaml`, `ticker/nudge.rhai`
- Create: `vantage-faker-stress/scenarios/churn/scenario.yaml`, `churn/ticket.rhai`
- Create: `vantage-faker-stress/scenarios/lifecycle/scenario.yaml`, `lifecycle/parcel.rhai`
- Create: `vantage-faker-stress/scenarios/swarm/scenario.yaml`, `swarm/aircraft.rhai`
- Create: `vantage-faker-stress/scenarios/sweeper/scenario.yaml`, `sweeper/tower.rhai`
- Create: `vantage-faker-stress/scenarios/warm/scenario.yaml` (includes `../lifecycle/parcel.rhai`)
- Test: `vantage-faker-stress/tests/smoke.rs`

**Interfaces:**
- Consumes: `scenario::load`, `Scenario::scaled`, and `runner::{run, RunOpts}`.

- [ ] **Step 1: Write the smoke test**

Create `vantage-faker-stress/tests/smoke.rs`:

```rust
//! Every scenario starts, samples and stops. Chaos scenarios need their
//! full duration for a verdict, so that test is ignored by default:
//! `cargo test --test smoke -- --ignored`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use vantage_faker_stress::runner::{RunOpts, run};
use vantage_faker_stress::scenario::load;
use vantage_faker_stress::verdict::{describe, verdict};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios")
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().join("scenario.yaml").exists())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

#[tokio::test(flavor = "multi_thread")]
async fn every_load_scenario_runs_briefly() {
    let names = names(&root());
    assert!(names.len() >= 7, "{names:?}");
    for name in names {
        let s = load(&root(), &name).unwrap().scaled(0.1, 0.1).without_warm();
        let out = run(&s, &RunOpts { duration: Duration::from_secs(2), dio: false }, |_| {})
            .await
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(!out.samples.is_empty(), "{name}: no samples");
        assert_eq!(out.panics, 0, "{name}: panicked");
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs each chaos scenario for its full duration"]
async fn every_chaos_scenario_is_contained() {
    let chaos = root().join("chaos");
    let mut failed = Vec::new();
    for name in names(&chaos) {
        let s = load(&root(), &format!("chaos/{name}")).unwrap();
        let expect = s.stress.expect.clone().expect("chaos scenarios set stress.expect");
        let out = run(&s, &RunOpts { duration: s.duration().unwrap(), dio: false }, |_| {})
            .await
            .unwrap();
        let result = verdict(&expect, &out);
        println!("chaos/{name}: {}", describe(&result));
        if result.is_err() {
            failed.push(name);
        }
    }
    assert!(failed.is_empty(), "not contained: {failed:?}");
}
```

The chaos test lists `chaos/` with the same `names()` helper. Task 8 adds those scenarios. Until then `names(&chaos)` would panic on a missing directory, so create an empty `scenarios/chaos/` in this task: add `scenarios/chaos/steady.rhai`, the shared healthy sim, now.

- [ ] **Step 2: Run the test and check it fails**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --test smoke 2>&1 | tail -20`
Expected: FAIL, because the `names.len() >= 7` assertion finds none.

- [ ] **Step 3: Write the scenarios**

`scenarios/idle/scenario.yaml`:

```yaml
# Parked threads and nothing else: what a sim costs while it sleeps.
seed: 1
tables:
  slot: { count: 0, columns: { note: {} } }
sims:
  sleeper:
    table: slot
    script: !include sleeper.rhai
    spawn: { burst: 200, max: 200 }
stress:
  duration: 20s
  limits: { cpu_pct: 400, rss_mb: 4096 }
```

`scenarios/idle/sleeper.rhai`:

```rhai
// Sleep forever, five minutes at a time.
loop {
    sleep(minutes(5));
}
```

`scenarios/ticker/scenario.yaml`:

```yaml
# One hot loop: a single sim nudging every row ten times a second.
seed: 2
tables:
  stock:
    count: 200
    columns:
      balance: { type: int, faker: { range: { min: 100, max: 1000 } } }
sims:
  ticker: { table: stock, script: !include nudge.rhai }
stress:
  duration: 20s
```

`scenarios/ticker/nudge.rhai`:

```rhai
// Move every balance by up to 5 either way, ten times a second.
loop {
    for id in ids() {
        let row = get(id);
        set(id, "balance", row.balance + rand_int(-5, 5));
    }
    sleep(seconds(0.1));
}
```

`scenarios/churn/scenario.yaml`:

```yaml
# The common app pattern: rows arrive, change a couple of times, leave.
seed: 7
tables:
  ticket:
    count: 200
    columns:
      status: { faker: { pick: { values: [Open, Pending, Closed], weights: [3, 1, 1] } } }
      amount: { type: int, faker: { range: { min: 1, max: 500 } } }
sims:
  ticket:
    table: ticket
    script: !include ticket.rhai
    clock: 10
    spawn: { burst: 100, rate: 30, max: 400 }
stress:
  duration: 30s
  limits: { cpu_pct: 400, event_lag_ms: 250 }
```

`scenarios/churn/ticket.rhai`:

```rhai
// One ticket: opened, worked on, closed, then gone.
let id = insert(#{ status: "Open", amount: rand_int(1, 500) });
sleep(seconds(rand_int(20, 120)));
patch(id, #{ status: "Pending" });
sleep(seconds(rand_int(20, 120)));
patch(id, #{ status: "Closed" });
sleep(seconds(30));
delete(id);
```

`scenarios/lifecycle/scenario.yaml`:

```yaml
# A realistic story across two tables: parcels and their tracking events.
seed: 11
tables:
  shipment: { count: 0, columns: { status: {}, hub: {} } }
  event: { count: 0, columns: { shipment: {}, what: {} } }
sims:
  parcel:
    table: shipment
    script: !include parcel.rhai
    clock: 60
    spawn: { burst: 50, rate: 10, max: 300 }
stress:
  duration: 30s
  limits: { cpu_pct: 400, event_lag_ms: 250 }
```

`scenarios/lifecycle/parcel.rhai`:

```rhai
// A parcel: booked, collected, moved through hubs, delivered, archived.
let hubs = ["LON", "MAN", "BHM", "GLA", "BRS"];
let events = [];
let id = insert(#{ status: "Booked", hub: pick(hubs) });
events.push(insert("event", #{ shipment: id, what: "Booked" }));

sleep(minutes(rand_int(5, 30)));
patch(id, #{ status: "Collected" });
events.push(insert("event", #{ shipment: id, what: "Collected" }));

for leg in 0..rand_int(1, 4) {
    sleep(minutes(rand_int(20, 90)));
    let hub = pick(hubs);
    patch(id, #{ status: "In transit", hub: hub });
    events.push(insert("event", #{ shipment: id, what: "Arrived " + hub }));
}

sleep(minutes(rand_int(10, 60)));
patch(id, #{ status: "Delivered" });
events.push(insert("event", #{ shipment: id, what: "Delivered" }));

sleep(hours(2));
for e in events {
    delete("event", e);
}
delete(id);
```

`scenarios/swarm/scenario.yaml`:

```yaml
# One sim per aircraft: a thread each, at the engine's cap.
seed: 21
tables:
  plane: { count: 0, columns: { lat: { type: float }, lon: { type: float } } }
sims:
  aircraft:
    table: plane
    script: !include aircraft.rhai
    clock: 10
    spawn: { burst: 1000, max: 1000 }
stress:
  duration: 30s
  limits: { cpu_pct: 400, event_lag_ms: 250 }
```

`scenarios/swarm/aircraft.rhai`:

```rhai
// One aircraft: its own row, a position report every 30 sim seconds.
let id = insert(#{ lat: rand_float(-60.0, 60.0), lon: rand_float(-180.0, 180.0) });
loop {
    sleep(seconds(30));
    let row = get(id);
    patch(id, #{ lat: row.lat + rand_float(-0.1, 0.1), lon: row.lon + rand_float(-0.1, 0.1) });
}
```

`scenarios/sweeper/scenario.yaml`:

```yaml
# The same load as swarm from a single sim: it ramps rows, not sims.
seed: 21
tables:
  plane:
    count: 1000
    columns:
      lat: { type: float, faker: { range: { min: -60, max: 60, decimals: 4 } } }
      lon: { type: float, faker: { range: { min: -180, max: 180, decimals: 4 } } }
sims:
  tower: { table: plane, script: !include tower.rhai, clock: 10 }
stress:
  duration: 30s
  limits: { cpu_pct: 400, event_lag_ms: 250 }
  ramp: { base: 1000, sims: false }
```

`scenarios/sweeper/tower.rhai`:

```rhai
// Every aircraft moves in one pass, every 30 sim seconds.
loop {
    for id in ids() {
        let row = get(id);
        patch(id, #{ lat: row.lat + rand_float(-0.1, 0.1), lon: row.lon + rand_float(-0.1, 0.1) });
    }
    sleep(seconds(30));
}
```

`scenarios/warm/scenario.yaml`:

```yaml
# lifecycle, opened twelve hours into its life: warm-start time and memory.
seed: 11
tables:
  shipment: { count: 0, columns: { status: {}, hub: {} } }
  event: { count: 0, columns: { shipment: {}, what: {} } }
sims:
  parcel:
    table: shipment
    script: !include ../lifecycle/parcel.rhai
    clock: 60
    warm: 12h
    spawn: { burst: 50, rate: 10, max: 300 }
stress:
  duration: 20s
```

`scenarios/chaos/steady.rhai`:

```rhai
// A well-behaved neighbour: one row, patched every second.
let id = insert(#{ note: "steady" });
loop {
    sleep(seconds(1));
    patch(id, #{ note: "tick " + rand_int(0, 1000) });
}
```

- [ ] **Step 4: Run the smoke test and check it passes**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --test smoke 2>&1 | tail -20`
Expected: `every_load_scenario_runs_briefly` passes, and the chaos test is listed as ignored.

If a scenario fails with a Rhai error in the logs (for example, float plus int arithmetic in `ticker`, or `rand_float` taking ints), fix the script. Don't loosen the test. Run `RUST_LOG=vantage_faker=debug cargo run -- run <name> --duration 5s` to see sim errors.

- [ ] **Step 5: Try a real run**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo run -- run churn --duration 8s --dio`
Expected: about 8 rows of samples, `live` somewhere in the tens, non-zero `events/s`, then a summary.

- [ ] **Step 6: Commit**

```bash
cd /Users/rw/Work/vantage-faker-rework
git add vantage-faker-stress/scenarios vantage-faker-stress/tests/smoke.rs
git commit -m "vantage-faker-stress: load scenarios and smoke test"
```

---

### Task 8: Chaos scenarios

**Files:**
- Create: `vantage-faker-stress/scenarios/chaos/{spin,throw,stale,flood,spawn-bomb,recurse}/scenario.yaml` and each one's `bad.rhai`

**Interfaces:**
- Consumes: `scenarios/chaos/steady.rhai` (Task 7), and the `stress.expect` and `stress.dio` fields (Task 2).

Every chaos scenario has the same table and the same `steady` neighbour. Only the `bad` sim, `duration` and `expect` differ.

- [ ] **Step 1: Write the six scenarios**

`chaos/spin/scenario.yaml`:

```yaml
# A sim that never sleeps. The operation budget (50M between sleeps) should end it.
seed: 31
tables:
  row: { count: 0, columns: { note: {} } }
sims:
  steady: { table: row, script: !include ../steady.rhai, spawn: { burst: 5, max: 5 } }
  bad: { table: row, script: !include bad.rhai }
stress:
  duration: 60s
  expect: { errored_min: 1 }
```

`chaos/spin/bad.rhai`:

```rhai
// Wait a moment, then spin without ever sleeping.
sleep(seconds(2));
let n = 0;
loop {
    n += 1;
}
```

`chaos/throw/scenario.yaml`:

```yaml
# Sims that throw on their third step while healthy ones keep writing.
seed: 32
tables:
  row: { count: 0, columns: { note: {} } }
sims:
  steady: { table: row, script: !include ../steady.rhai, spawn: { burst: 5, max: 5 } }
  bad: { table: row, script: !include bad.rhai, spawn: { burst: 5, max: 5 } }
stress:
  duration: 10s
  expect: { errored_min: 5 }
```

`chaos/throw/bad.rhai`:

```rhai
// Two good steps, then an uncaught exception.
sleep(seconds(1));
sleep(seconds(1));
throw "chaos: failing on the third step";
```

`chaos/stale/scenario.yaml`:

```yaml
# Writes to rows that are already gone. Nothing should panic or count as a write.
seed: 33
tables:
  row: { count: 0, columns: { note: {} } }
sims:
  steady: { table: row, script: !include ../steady.rhai, spawn: { burst: 5, max: 5 } }
  bad: { table: row, script: !include bad.rhai, spawn: { burst: 20, max: 20 } }
stress:
  duration: 10s
  expect: { errored_min: 0 }
```

`chaos/stale/bad.rhai`:

```rhai
// Create a row, delete it, then keep writing to it.
loop {
    let id = insert(#{ note: "doomed" });
    delete(id);
    sleep(seconds(1));
    patch(id, #{ note: "late" });
    set(id, "note", "later");
    delete(id);
}
```

`chaos/flood/scenario.yaml`:

```yaml
# Ten thousand inserts with no pause, against a Dio. Watch `lagged` and the lag.
seed: 34
tables:
  row: { count: 0, columns: { note: {} } }
sims:
  steady: { table: row, script: !include ../steady.rhai, spawn: { burst: 5, max: 5 } }
  bad: { table: row, script: !include bad.rhai }
stress:
  duration: 15s
  dio: true
  expect: { errored_min: 0 }
```

`chaos/flood/bad.rhai`:

```rhai
// Wait a moment, then insert 10k rows in one burst.
sleep(seconds(2));
for i in 0..10000 {
    insert(#{ note: "flood " + i });
}
```

`chaos/spawn-bomb/scenario.yaml`:

```yaml
# Every sim starts two more. `max` should hold the thread count flat.
seed: 35
tables:
  row: { count: 0, columns: { note: {} } }
sims:
  steady: { table: row, script: !include ../steady.rhai, spawn: { burst: 5, max: 5 } }
  bomb: { table: row, script: !include bad.rhai, spawn: { burst: 1, max: 200 } }
stress:
  duration: 15s
  expect: { errored_min: 0, max_threads: 320 }
```

`chaos/spawn-bomb/bad.rhai`:

```rhai
// Start two more of myself, then idle.
spawn_sim("bomb", #{});
spawn_sim("bomb", #{});
loop {
    sleep(minutes(10));
}
```

`chaos/recurse/scenario.yaml`:

```yaml
# Unbounded recursion. The call-level limit should end it before the stack does.
seed: 36
tables:
  row: { count: 0, columns: { note: {} } }
sims:
  steady: { table: row, script: !include ../steady.rhai, spawn: { burst: 5, max: 5 } }
  bad: { table: row, script: !include bad.rhai }
stress:
  duration: 10s
  expect: { errored_min: 1 }
```

`chaos/recurse/bad.rhai`:

```rhai
// Recurse until something stops it.
fn dive(n) {
    dive(n + 1)
}
sleep(seconds(1));
dive(0);
```

`max_threads: 320` allows for 205 sim threads, the driver, the tokio worker and blocking threads, and the test harness. If the first run shows the baseline process uses more, raise the number to (non-sim threads + 205 + 20) and explain the value in the YAML comment.

- [ ] **Step 2: Check they load and list**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo run -- list`
Expected: 13 names: `chaos/flood`, `chaos/recurse`, `chaos/spawn-bomb`, `chaos/spin`, `chaos/stale`, `chaos/throw`, `churn`, `idle`, `lifecycle`, `sweeper`, `swarm`, `ticker`, `warm`.

- [ ] **Step 3: Run the chaos test**

Run: `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test --test smoke -- --ignored --nocapture 2>&1 | grep -E "chaos/|test result"`
Expected: one verdict line per scenario.

**A `NOT contained` verdict on 0.7 is a finding, not a failure of this task.** Record each one in the findings list in Task 10. Only change a scenario if it is wrong, for example if its script errors for a reason unrelated to the misbehaviour it is meant to show.

- [ ] **Step 4: Commit**

```bash
cd /Users/rw/Work/vantage-faker-rework
git add vantage-faker-stress/scenarios/chaos
git commit -m "vantage-faker-stress: chaos scenarios for spin, throw, stale, flood, spawn-bomb, recurse"
```

---

### Task 9: README

**Files:**
- Create: `vantage-faker-stress/README.md`

- [ ] **Step 1: Write the README**

Follow the outline in SPEC.md ("README outline"). Required content:

1. **What it is for.** Relative numbers for faker sims: capacity, cost per sim, event flow, and containment of misbehaving sims. It is not a benchmark suite.
2. **Quick start:**
   ```
   cd vantage-faker-stress
   cargo run -- list
   cargo run -- run churn
   cargo run -- run churn --dio --json /tmp/churn.json
   cargo run -- ramp swarm --steps 100,250,500,1000 --hold 15s
   cargo run -- compare baseline-0.7.json /tmp/churn.json
   ```
3. **CLI reference.** Every flag from `main.rs`, and what `--scale`, `--dio` and `--warm` change.
4. **Reading the output.** A table of the sample columns (copy the one in SPEC.md "Metrics") and how the lag estimate works (backlog ÷ apply rate). Cost per sim is averaged over the second half of the run. CPU is a percentage of one core.
5. **Writing a scenario.** An annotated copy of `scenarios/churn/scenario.yaml`. Note that columns are `{ type, faker }` as in vantage-ui table files, that `!include` resolves from the scenario's directory within `scenarios/`, and what the `stress:` keys are (`duration`, `limits`, `ramp`, `expect`, `dio`).
6. **Chaos scenarios.** What each one does, what `contained` means (the verdict rules from `verdict.rs`), and how to run them: `cargo test --test smoke -- --ignored --nocapture`, or `cargo run -- run chaos/spin`.
7. **Comparing versions.** Record a JSON report on each version and run `compare`. Both runs must use the same profile.
8. **Known limits:**
   - `MAX_LIVE` is 1000 per engine.
   - Debug builds are the default; add `--release` for absolute numbers.
   - The thread count includes tokio and harness threads.
   - Process CPU is shared by everything in the process, so don't run two scenarios at once.
9. **Baseline (0.7).** Filled in by Task 10.

Keep it plain and direct. Don't include a feature-list preamble or marketing tone.

- [ ] **Step 2: Commit**

```bash
cd /Users/rw/Work/vantage-faker-rework
git add vantage-faker-stress/README.md
git commit -m "vantage-faker-stress: README"
```

---

### Task 10: Baseline run, findings, and spec sync

**Files:**
- Create: `vantage-faker-stress/baseline-0.7.json`
- Modify: `vantage-faker-stress/README.md` (the "Baseline (0.7)" section)
- Modify: `vantage-faker-stress/SPEC.md` (fold in the seven spec deltas listed at the top of this plan)

- [ ] **Step 1: Record the runs**

Run each of these as a separate command, in a debug build, with nothing else heavy running:

```
cargo run -- ramp idle --steps 100,250,500,1000 --hold 15s --json /tmp/b-idle.json
cargo run -- ramp churn --steps 100,200,400,800 --hold 20s --dio --json /tmp/b-churn.json
cargo run -- ramp swarm --steps 100,250,500,1000 --hold 20s --dio --json /tmp/b-swarm.json
cargo run -- ramp sweeper --steps 100,250,500,1000 --hold 20s --dio --json /tmp/b-sweeper.json
cargo run -- run warm --json /tmp/b-warm.json
cargo test --test smoke -- --ignored --nocapture
```

Combine the five JSON reports into `baseline-0.7.json` as a JSON array of reports, in the order above. Add a `faker-stress compare` note in the README explaining that `compare` takes single reports, so you extract one element to compare (for example with `jq '.[1]'`).

- [ ] **Step 2: Write the baseline section**

In the README's "Baseline (0.7)" section, write 5 to 10 short lines:
- RSS and CPU per idle sim.
- The highest churn step inside the limits.
- swarm against sweeper at 1000 rows: CPU, threads and lag.
- Warm-start time for `warm`.
- The chaos verdicts, with each `NOT contained` reason quoted.

Put the findings for sub-project 2 under a sub-heading "Findings for 0.8". Expected candidates, which you confirm or drop from the actual output:
- How long `chaos/spin` pins a core before the budget stops it.
- Whether `chaos/stale` produces phantom `Deleted` events. `FakerCtx::expire` broadcasts even for missing ids, so `events/s` would exceed `writes/s`.
- `lagged` under `chaos/flood`.

- [ ] **Step 3: Sync SPEC.md**

Edit SPEC.md so it matches what was built. Apply each item in "Spec deltas" at the top of this plan to the matching SPEC.md section:
- Scenario format: the column shape and `!include` scope.
- CLI: ramp step semantics and `stress.ramp`.
- Chaos: the verdict window, the flood expectation, and `stress.dio`.
- Testing: the ignored chaos test.

Update the chaos table's "Expected on 0.7" column where a finding changed it.

- [ ] **Step 4: Commit**

```bash
cd /Users/rw/Work/vantage-faker-rework
git add vantage-faker-stress
git commit -m "vantage-faker-stress: 0.7 baseline, findings, spec synced with the build"
```

- [ ] **Step 5: Final checks**

Run each separately:
- `cd /Users/rw/Work/vantage-faker-rework/vantage-faker && cargo test --features rhai 2>&1 | tail -5`
- `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo test 2>&1 | tail -5`
- `cd /Users/rw/Work/vantage-faker-rework/vantage-faker-stress && cargo build 2>&1 | grep -c warning`

Expected: both test suites pass, and there are 0 warnings in the harness crate.
