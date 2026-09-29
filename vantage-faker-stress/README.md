# vantage-faker-stress

A stress harness for vantage-faker sim scenarios. It runs a scenario's sims
against faker tables and reports what they cost: how many sims one engine
can carry, what each sim costs in CPU, memory and threads, how fast writes
turn into events and how far a subscriber falls behind, and whether a
misbehaving sim stays contained.

The numbers are relative, not absolute. Use them to compare scenarios
against each other or the same scenario across faker versions, on the same
machine and build profile. This is not a benchmark suite, and sims aren't
written to be efficient — faker is a testing tool.

## Quick start

```
cd vantage-faker-stress
cargo run -- list
cargo run -- run churn
cargo run -- run churn --dio --json /tmp/churn.json
cargo run -- ramp swarm --steps 100,250,500,1000 --hold 15s
cargo run -- compare baseline-0.7.json /tmp/churn.json
```

## CLI reference

### `list`

Prints the scenario names under `scenarios/`, one per line: `churn`,
`chaos/spin`, and so on. A name with a `/` is a scenario nested one
directory down, which is how the chaos scenarios are organized.

### `run <scenario>`

| Flag | Default | What it does |
|---|---|---|
| `--duration <dur>` | the scenario's `stress.duration` | how long to sample |
| `--scale <N>` | `1.0` | multiplies every sim's `burst` and `max`, and every table's `count`, by `N`. Decimals are allowed; each result is rounded and kept at least 1 |
| `--dio` | off | attaches a `Dio` to every table, so lag and event throughput measure `Dio::handle_event` and re-list cost, not just the raw broadcast |
| `--json <path>` | none | writes the run's report to a JSON file |

`run` always applies the scenario's `warm:` settings, if it has any.

### `ramp <scenario>`

| Flag | Default | What it does |
|---|---|---|
| `--steps <list>` | `50,100,200,400,800` | comma-separated live-sim targets. Each step runs on a fresh engine |
| `--hold <dur>` | `20s` | how long each step runs before the next one starts |
| `--dio` | off | same as `run --dio` |
| `--warm` | off | keeps the scenario's `warm:` settings. By default `ramp` strips them so every step starts cold |
| `--json <path>` | none | writes every step's report to a JSON file |

`ramp` stops early, before running the step that broke it: at the first step
whose summary crosses a `stress.limits` threshold, or before a step whose
scaled sim count would exceed `MAX_LIVE` (1000). The reason is printed and
recorded on the last completed step.

### `compare <a.json> <b.json>`

Prints both reports' summaries side by side, step by step, with the percent
change from `a` to `b`. Only meaningful when both reports come from the same
scenario, the same mode (`run` or `ramp`), and the same build profile.

## Reading the output

Each sample is one row, printed once a second:

| Column | Source | Meaning |
|---|---|---|
| `t` | wall clock | seconds since the run started |
| `live` | `SimEngine::stats().live` | sims running right now |
| `spawned` / `ended` / `errored` | `stats()` | running totals |
| `threads` | OS thread count for the process | every thread, not just sim threads |
| `cpu%` | process CPU | percent of one core; 400 means four cores busy |
| `rss` | process memory | resident memory, MB |
| `writes/s` | `stats().writes`, delta | data-verb writes that landed on a table |
| `events/s` | counting subscriber on each table's broadcast | change events delivered |
| `lagged` | subscribers' `RecvError::Lagged` | events dropped because a subscriber fell behind |
| `lag ms` | see below | how far event consumers trail the writers |

Each table has one subscriber task that tracks its own backlog (queued,
undelivered events) and its own delivery rate, and estimates
`lag ms = backlog ÷ apply rate`. Without `--dio` the rate is raw broadcast
receives; with `--dio` it's events actually applied to a `Dio`, so `--dio`
lag reflects the Dio's apply and re-list cost on top of the broadcast.

At the end of a run the harness prints peak and mean of each column, the
warm-start time when the scenario sets `warm:`, and cost per sim: CPU
percent and RSS divided by the live count, averaged over the second half of
the run (so a scenario that's still ramping up doesn't skew the number).

## Writing a scenario

`scenarios/churn/scenario.yaml`, annotated:

```yaml
seed: 7                       # baked into the run, for repeatable numbers

tables:
  ticket:
    count: 200                # rows created before any sim starts
    columns:
      status: { faker: { pick: { values: [Open, Pending, Closed], weights: [3, 1, 1] } } }
      amount: { type: int, faker: { range: { min: 1, max: 500 } } }

sims:
  ticket:
    table: ticket              # which table this sim's insert/patch/delete hit
    script: !include ticket.rhai
    clock: 10                  # sim-seconds per wall-clock second
    spawn: { burst: 100, rate: 30, max: 400 }

stress:
  duration: 30s
  limits: { cpu_pct: 400, event_lag_ms: 250 }
```

- `tables.<name>.columns.<col>` is `{ type, faker }`, the same shape a
  vantage-ui table file uses. `type` defaults to `string`. `faker` is a
  `ColumnGen` (`pick`, `range`, and the rest). A column with no generator,
  `{}`, falls back to `ValueGen`'s heuristics by column name and type.
- `!include <path>` reads a file relative to the scenario's own directory
  (here, `scenarios/churn/`) and can't resolve outside `scenarios/`.
- `sims.<name>` takes the same fields as vantage-ui's `SimSpec`/`SpawnSpec`:
  `table`, `script`, `clock`, `warm`, and `spawn { burst, rate, max, args }`.
- `stress:` keys:
  - `duration`: how long a plain `run` samples for, unless `--duration`
    overrides it.
  - `limits`: thresholds `ramp` stops at: `cpu_pct`, `event_lag_ms`,
    `rss_mb`.
  - `ramp`: what one ramp step unit means. `base` is the unit count at
    scale 1, defaulting to the sum of every sim's `max`. `sims: false`
    ramps table rows instead of sim counts, as `sweeper` does.
  - `expect`: chaos scenarios only, `errored_min` and optionally
    `max_threads`. See Chaos scenarios below.
  - `dio`: attaches a `Dio` to every table, same as always passing `--dio`.

## Chaos scenarios

Each chaos scenario runs one misbehaving sim next to a well-behaved
`steady` sim, so the verdict also confirms the healthy neighbour kept
working.

| Scenario | Misbehaviour | Expected on 0.7 |
|---|---|---|
| `chaos/spin` | `loop {}` with no sleep | the operation budget ends it (`errored` +1) |
| `chaos/throw` | throws on its third step | it ends and is counted; the healthy sims keep writing |
| `chaos/stale` | patches and deletes ids another sim already deleted | no panic; stale writes don't count as `writes` |
| `chaos/flood` | 10k inserts with no sleep | the budget ends it; `lagged` and the Dio re-list cost are visible |
| `chaos/spawn-bomb` | each sim spawns two copies of itself | bounded by `max` and `MAX_LIVE`; the thread count levels off |
| `chaos/recurse` | unbounded recursion | the call-depth limit ends it; no stack overflow |

A run is `contained` when all of these hold:

- CPU, averaged over the last 3 samples, is back within 10 points of the
  pre-run baseline.
- `errored` reached the scenario's `expect.errored_min`.
- if `expect.max_threads` is set, the thread count never peaked above it.
- nothing in the process panicked.

Otherwise the verdict is `NOT contained: <reasons>`, naming every check
that failed, not just the first one.

Run them with:

- `cargo test --test smoke -- --ignored --nocapture` runs every scenario
  under `scenarios/chaos/` for its full `stress.duration` and asserts each
  one comes back `contained`.
- `cargo run -- run chaos/spin` runs one directly and prints its verdict
  at the end.

## Comparing versions

Record a JSON report for the same scenario on each faker version, then
compare them:

```
cargo run -- run churn --json v1.json    # on version A
cargo run -- run churn --json v2.json    # on version B
cargo run -- compare v1.json v2.json
```

Both runs need to be the same scenario, the same mode (`run` or `ramp`),
and the same build profile — `compare` doesn't adjust for a debug/release
difference, and neither does the harness.

## Known limits

- `MAX_LIVE` is 1000 live sims per engine; `ramp` stops before a step that
  would need more.
- Debug builds are the default, for fast rebuilds while iterating. Numbers
  from a debug build aren't comparable to a release build's — pass
  `--release` when you need absolute numbers.
- `threads` counts every OS thread in the process, including tokio's
  worker threads and the harness's own, not just sim threads.
- Process CPU and memory are shared by everything running in the process.
  Don't run two scenarios at once.

## Baseline (0.7)

Filled in from `baseline-0.7.json`, recorded on the 0.7 engine.
