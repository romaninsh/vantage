# vantage-faker-stress — design spec

Date: 2026-09-29 · Branch: `faker/rework` · Status: draft for review

## Context

This is sub-project 0 of the faker rework. The full sequence is:

0. **Stress harness** (this spec)
1. Full memory datasource
2. vantage-faker 0.8: pluggable storage, a single build path, write events, the `sim` feature, idempotent writes and sim isolation
3. Port the remaining mutators (fifo, rhai effect, flights, pulse, live_folder) to Rhai sims, then migrate vantage-ui and the example apps
4. vantage-ui action kind that runs Rhai or spawns sims
5. Hospital sim game demo

The harness comes first. It records a baseline on the 0.7 engine and gets re-run after sub-projects 1 and 2. It answers four questions:

- How many sims can one engine run?
- What does each sim cost in CPU, memory and threads?
- How fast do writes turn into events, and how far behind does a subscribed Dio fall?
- Does a misbehaving sim stay contained?

It is a developer tool that other people can pick up from its README. Readable, comparable numbers matter more than precision. Sims are not expected to be efficient, because faker is a testing tool.

Settled decisions this spec relies on:

- Sims are linear Rhai scripts, one OS thread each. There will be no step engine.
- Two sim styles are both supported and compared: one sim per row (thread-heavy) and one sim looping over many rows (single thread).
- Scenarios use the same YAML shape as a vantage-ui faker datasource. What the harness measures is what apps run.

## Crate layout

`vantage-faker-stress/` is a standalone crate at the vantage repo root and is added to the root workspace `exclude` list, like `vantage-faker` and `test-api-server`.

```
vantage-faker-stress/
  README.md
  SPEC.md                        this file
  Cargo.toml
  baseline-0.7.json              first recorded run (see Deliverables)
  scenarios/<name>/scenario.yaml   + the .rhai files it includes
  scenarios/chaos/<name>/scenario.yaml
  src/lib.rs                     module list                                     ~10 LOC
  src/main.rs                    CLI parsing and dispatch                        ~240
  src/scenario.rs                scenario struct: tables, sims, stress, loading  ~160
  src/scenario/build.rs          durations, --scale/ramp scaling, SimDefs, FakerColumns  ~120
  src/scenario/include.rs        !include resolution and confinement to scenarios/  ~50
  src/scenario/tests.rs          unit tests for the three files above            ~180
  src/runner.rs                  builds FakerTables and the SimEngine, runs one pass  ~100
  src/sampler.rs                 samples CPU, RSS, threads and counters once a second  ~170
  src/load.rs                    table consumers: counting subscriber, optional Dio  ~170
  src/load/drain.rs              drain-time probe behind `lag ms`                ~135
  src/load/tests.rs              subscriber and lag tests                        ~85
  src/report.rs                  live table, final summary, JSON report          ~170
  src/report/compare.rs          two reports side by side with per-metric deltas ~55
  src/report/tests.rs            unit tests for summary maths and compare        ~70
  src/ramp.rs                    ramp step scaling and stress.limits stop conditions  ~105
  src/verdict.rs                 chaos containment verdict                       ~110
  src/panics.rs                  process-wide panic counter                      ~25
  src/threads.rs                 OS thread count (sysinfo, or the kernel directly on macOS)  ~40
  tests/runner.rs                integration test: a short run samples live sims and events  ~40
  tests/smoke.rs                 every load scenario runs briefly; chaos suite (ignored)  ~80
```

Dependencies: `vantage-faker` (path `../vantage-faker`, feature `rhai`), `vantage-diorama`, `vantage-vista`, `vantage-core`, `vantage-dataset`, `serde`, `serde_yaml_ng`, `serde_json`, `indexmap`, `clap`, `sysinfo`, `libc`, `tokio`, `tracing-subscriber`, `tempfile`.

## CLI

```
faker-stress list
faker-stress run  <scenario> [--duration 30s] [--scale N] [--dio] [--json out.json]
faker-stress ramp <scenario> [--steps 50,100,200,400,800] [--hold 20s] [--dio] [--warm] [--json out.json]
faker-stress compare <a.json> <b.json>
```

- `<scenario>` is a path under `scenarios/`, for example `churn` or `chaos/spin`.
- `run` does one pass for the scenario's `stress.duration`, or for `--duration` if given.
- `--scale N` multiplies every `burst` and `max` in the scenario, and the table `count`s. Decimals are allowed; each result is rounded and kept at least 1.
- `--dio` attaches a Dio to every table (see Metrics).
- `run` always honours the scenario's `warm:` settings. `ramp` strips them so each step starts cold, unless `--warm` is passed.
- `ramp` runs one fresh engine per step. Step `N` applies a scale factor `N / base`: by default `base` is the sum of every def's `max`, and the factor scales `burst`, `max` and table `count` — the same as `--scale`. A scenario may set `stress.ramp: { base: <n>, sims: false }` to scale only the table `count`s, leaving sim counts alone; `sweeper` uses this, because it ramps rows, not sims. A step is held for `--hold`, then stopped before the next step starts. `ramp` stops early at the first step that breaks a `stress.limits` value, or when the next step would go past `MAX_LIVE` (1000). The reason it stopped is part of the report.
- `compare` prints two reports side by side with per-metric deltas. It is meant for comparing the same scenario across faker versions.

## Scenario format

A scenario uses the vantage-ui faker datasource shape. It adds `tables:`, which an app gets from its table files instead, and a `stress:` block.

```yaml
# scenarios/churn/scenario.yaml
seed: 7
tables:
  ticket:
    count: 200
    columns:
      status: { faker: { pick: { values: [Open, Pending, Closed], weights: [3, 1, 1] } } }
      amount: { type: int, faker: { range: { min: 1, max: 500 } } }
sims:
  churn:
    table: ticket
    script: !include churn.rhai
    clock: 10
    spawn: { burst: 100, rate: 30, max: 400 }
stress:
  duration: 30s
  limits: { cpu_pct: 400, event_lag_ms: 250 }
```

- `tables.<name>.columns.<col>` is `{ type?, faker? }`, the same shape a vantage-ui table column uses. `type` defaults to `string`. `faker` deserializes into `vantage_faker::ColumnGen` (`pick`, `range`, and the rest); a column with no generator, `{}`, falls back to `ValueGen`'s heuristics by column name and type. It is not a bare generator map — `note: {}` reads as "no generator", not as an empty `ColumnGen`.
- `sims.<name>` has the same fields as vantage-ui's `SimSpec`/`SpawnSpec`: `table`, `script`, `clock`, `warm`, and `spawn { burst, rate, max, args }`. The harness keeps its own copy of those serde types, about 40 lines. Sub-project 2 moves them into vantage-faker behind a `serde` feature, and the harness then drops its copy.
- `!include` resolves relative to the scenario's own directory, but may reach anywhere under `scenarios/` — `chaos/*` scenarios share `chaos/steady.rhai`, and `warm` reuses `lifecycle/parcel.rhai`. It can't resolve outside `scenarios/`.
- Durations use the same format as vantage-ui (`30s`, `5m`, `12h`, `3d`).
- `stress.limits` sets the thresholds `ramp` stops at. Supported keys are `cpu_pct`, `event_lag_ms` and `rss_mb`.
- `stress.dio: true` attaches a Dio to every table, the same as always passing `--dio`. `chaos/flood` sets it.

## Metrics

A sampler reads everything once a second and prints one row per sample:

| Column | Source | Meaning |
|---|---|---|
| `t` | wall clock | seconds since the live run started |
| `live` | `SimEngine::stats().live` | sims running right now |
| `spawned` / `ended` / `errored` | `stats()` | running totals |
| `threads` | `sysinfo`, process thread count | OS threads in the whole process |
| `cpu%` | `sysinfo`, process CPU | % of one core; 400 means four cores |
| `rss` | `sysinfo` | resident memory, MB |
| `writes/s` | `stats().writes`, delta | data-verb writes that went to a table |
| `events/s` | counting subscriber on each table's broadcast | change events that were delivered |
| `lagged` | subscribers' `RecvError::Lagged` | events dropped because a subscriber fell behind |
| `lag ms` | see below | how far event consumers trail the writers |

Lag is measured the same way with or without `--dio`: a drain-time probe per table. Each table has one subscriber task, which counts every event it receives, plus `n` for each `Lagged(n)` (skipped events count as drained). At each sample, for each table: if a probe is armed, lag is the time since it was armed and the probe stays armed; otherwise lag is the last recorded drain time (0 if the table had no backlog at the previous sample). Then, if the backlog (`Sender::len()`) is above 0 and no probe is armed, a probe is armed with target "events seen so far + backlog" and the current time; with no backlog, the recorded drain time resets to 0. The subscriber checks the probe after each receive and, once it has seen the target, records the elapsed time and disarms it. A drain is therefore reported one tick late, and a consumer that never catches up shows lag growing each tick. The sample's `lag ms` is the maximum over tables. Without `--dio`, the subscriber only counts events. With `--dio`, it calls `dio.handle_event` for each one before counting it. So `--dio` shows the cost of the Dio's apply and re-list work on top of the raw broadcast.

With `--dio`, the Dio's lens lists the master table into its cache once, on start (`on_start` calls `list_values` then `insert_values`); a table's seeded rows reach the cache this way, not as broadcast events. From then on the lens applies each `ChangeEvent` to the cache directly.

At the end of a run the harness prints a summary: peak and mean of each column, warm-start time (when warm is on), steady-state cost per sim (`rss / live` and `cpu% / live`, averaged over the last half of the run), and the chaos verdict when there is one.

`--json` writes the summary, every sample, the scenario name, the git revision of the vantage checkout, the build profile, and the vantage-faker version.

### Engine addition

`vantage-faker` gains one read-only method:

```rust
impl SimEngine {
    pub fn stats(&self) -> SimStats;
}
pub struct SimStats { pub live: usize, pub spawned: u64, pub ended: u64, pub errored: u64, pub writes: u64 }
```

- `live` and `spawned` come from state the scheduler already keeps.
- `ended` counts scripts that finished or called `done()`.
- `errored` counts scripts that stopped because of a Rhai error (budget, call-depth and expression-depth limits, and thrown exceptions) or because a sim's thread panicked.
- `writes` counts `insert`, `set`, `patch` and `delete` calls that changed a table, as atomic increments in the data vocabulary.

The method stays in 0.8 so that `compare` keeps working across versions. It also gets a CHANGELOG line in vantage-faker. That line goes in the 0.7.0 block if PR #405 hasn't been released when this merges, and in a new patch block if it has.

## Scenarios

### Load

| Scenario | Sims | Measures |
|---|---|---|
| `idle` | `loop { sleep(minutes(5)) }`, burst 200 | Cost of parked threads alone: RSS and threads per sim |
| `ticker` | 1 sim; patches every row of a 200-row table by ±5 every 100 ms | One hot loop; throughput of a single thread |
| `churn` | Spawner; each sim runs `insert → sleep(rand) → patch → delete` | The typical app pattern; events/s and Dio lag |
| `lifecycle` | Shipment-like: 5–8 sleeps and patches across 2 tables; about 20 lines | A realistic mix at scale |
| `swarm` | One sim per row, 1000 rows, `patch` every 30 sim-s at clock 10 | One-thread-per-sim at the `MAX_LIVE` cap (the flights case) |
| `sweeper` | 1 sim that loops over the same 1000 rows every 30 sim-s | The same load as `swarm` on one thread |
| `warm` | `lifecycle` with `warm: 12h` | Warm-start time and peak memory |

`swarm` and `sweeper` write the same data at the same rate. Their pair of reports is the thread-per-sim versus single-loop comparison.

### Chaos

A chaos scenario has a `stress.expect:` block with `errored_min` (the fewest sims that must end in error) and optionally `max_threads` (a ceiling the thread count must level off under). The run ends with a verdict:

- `contained`: the process stayed up, `errored` reached `errored_min`, the thread count stayed under `max_threads`, and CPU — averaged over the last 3 samples of the run — is within 10 points of the 1 s idle baseline taken before the engine started. The run lasts the scenario's `stress.duration`.
- `NOT contained: <reason>`: anything else. Examples: CPU stays pinned, the thread count keeps growing, a panic is logged, or the run times out.

| Scenario | Misbehaviour | Expected on 0.7 |
|---|---|---|
| `chaos/spin` | `loop {}` with no sleep | The operation budget ends it (`errored` +1) |
| `chaos/throw` | `throw` on its third step, while healthy sims run beside it | It ends and is counted; the healthy sims keep writing |
| `chaos/stale` | Patches and deletes ids that another sim has already deleted | No panic; stale writes don't count as `writes` |
| `chaos/flood` | 10k inserts with no sleep | Well under the 50M-operation budget, so it completes normally (`errored_min: 0`); the finding to look for is `lagged`, the lag estimate and the consumer's re-list, not an error |
| `chaos/spawn-bomb` | Each sim `spawn_sim`s two copies of itself | Bounded by `max` and `MAX_LIVE`; the thread count levels off |
| `chaos/recurse` | Unbounded recursion | The call-level limit ends it; no stack overflow |

On 0.7, a result that differs from the expected one is recorded as a finding, not fixed here. The findings feed the isolation and idempotency design in sub-project 2. Confirmed findings from the 0.7 baseline: `chaos/stale` shows `events/s` above `writes/s`, because `FakerCtx::expire` broadcasts a `Deleted` event even when the id is already gone; `chaos/flood` completes normally but the consumer falls far behind (`lagged` in the thousands, peak lag in the tens of seconds).

## README outline

1. What the harness is for, and what it isn't: numbers are relative, not benchmarks.
2. Quick start: `cargo run -- run churn`.
3. CLI reference.
4. Reading the output: each column, the lag estimate, the cost per sim, and why CPU can exceed 100.
5. Writing a scenario: its format, `!include`, and `stress:` limits and expectations.
6. Chaos scenarios and verdicts.
7. Comparing runs across faker versions.
8. Known limits: `MAX_LIVE`, debug builds by default (relative numbers, fast rebuilds; pass `--release` for absolute numbers), and macOS thread counts including runtime threads.
9. Baseline: what the 0.7 run showed, in a few lines.

## Testing

- Unit tests in `scenario.rs`: parsing, `!include` resolution and confinement, durations, `--scale` rounding, and errors that name the file and key for unknown fields.
- Unit tests in `report.rs`: summary maths (peak, mean, cost per sim), and `compare` on two fixture JSON files.
- `tests/smoke.rs`, `every_load_scenario_runs_briefly`: runs every scenario directly under `scenarios/` (the load scenarios; `chaos/` itself has no `scenario.yaml`, so its nested scenarios aren't picked up here) for 2 s at `--scale 0.1` on the system clock. It checks that each one starts, produces at least one sample and stops cleanly, without a panic. It never asserts absolute numbers.
- `tests/smoke.rs`, `every_chaos_scenario_is_contained`: `#[ignore]`d, since a chaos scenario needs its full `stress.duration` to reach a verdict and each run measures the whole process's CPU. Runs on request, one scenario at a time: `cargo test --test smoke -- --ignored --nocapture`. Asserts every `scenarios/chaos/*` scenario comes back `contained`.
- vantage-faker gets tests for `SimEngine::stats()` (live, ended, errored and writes counts) next to the existing sim tests.

## Deliverables

1. The `vantage-faker-stress` crate, its README, and the scenarios listed above.
2. `SimEngine::stats()` in vantage-faker, with tests and a CHANGELOG line.
3. `baseline-0.7.json`: `ramp` on `idle`, `churn`, `swarm` and `sweeper`, a `run` of `warm` and of `chaos/flood`, all on a debug build, with a short summary in the README. The full chaos suite (`cargo test --test smoke -- --ignored`) is run separately and its verdicts recorded in the README, not saved to the JSON.

## Out of scope

- Any fix to engine behaviour that a chaos run exposes. Those go to sub-project 2.
- Storage backends other than faker's in-memory store. The harness gains a storage option once sub-projects 1 and 2 exist.
- GPUI rendering cost. The harness measures the engine, the broadcast and the Dio, not vantage-ui frames.
- CI thresholds. They can be added later over the same scenarios.
