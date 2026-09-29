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
| `--scale <N>` | `1.0` | multiplies every sim's `burst`, `rate` and `max`, and every table's `count`, by `N`. Decimals are allowed. `burst`, `max` and `count` are rounded, a zero stays 0, and every other result is kept at least 1; `rate` is not rounded, and a sim with no `rate` keeps none |
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

Each step scales the scenario the way `--scale` does, by the step's target
divided by `stress.ramp.base`, so `burst`, `rate`, `max` and row counts
grow together. With `stress.ramp.sims: false` only row counts are scaled.

`ramp` stops early in two ways. If a step's scaled sim count would exceed
`MAX_LIVE` (1000), that's caught before the step runs, so it never runs at
all; the reason is printed and recorded on the last step that did
complete. If a step's own summary crosses a `stress.limits` threshold,
that's caught after the step finishes; the step is recorded with the
reason attached, and no further steps run.

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

`lag ms` is a drain-time probe. Each table has one subscriber task. When a
sample finds events queued for it, the harness notes the backlog and the
time, then waits for the subscriber to get through that many events
(events it skips with `Lagged` count as got through). The time that took
shows up in the next sample, so the column runs one tick late. If the
subscriber still hasn't caught up by the next sample, the column shows the
time waited so far, so a consumer that never catches up shows lag growing
by about 1000 ms a tick. A tick with nothing queued reads 0 on the tick
after it. The column is the slowest table's figure, not a sum. Without
`--dio` the subscriber only counts events; with `--dio` it applies each one
to a `Dio`, so `--dio` lag includes the Dio's apply and re-list cost on top
of the broadcast.

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
| `chaos/stale` | inserts a row, deletes it, then keeps patching, setting and deleting that same gone id | no panic; stale writes don't count as `writes` |
| `chaos/flood` | 10k inserts with no sleep | well under the operation budget, so it completes normally (`errored_min: 0`); watch `lagged`, the lag estimate and the Dio re-list cost |
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

`compare` takes two single reports, but `baseline-0.7.json` is a JSON array
of six (one per scenario recorded for the baseline). Pull the one you want
out first, for example `jq '.[1]' baseline-0.7.json > churn-0.7.json`, then
compare that against a fresh run.

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

Recorded in `baseline-0.7.json`, a debug build, one scenario at a time.

- `idle` at 1000 parked sims: 98 MB peak RSS, 1018 threads, peak CPU under 6%.
- `churn`'s ramp reaches its top step, 800 live sims, without breaching
  either limit: mean CPU 25%, peak 90%, peak lag 201 ms.
- `swarm` and `sweeper` both stop their ramp after the first step (100
  live/rows): every def writes on the same clock tick, and the per-second
  `lag ms` estimate spikes to the tens of seconds for that one sample
  (63000 ms / 59000 ms) even though `lagged` stays 0, tripping the 250 ms
  limit. At the step both reach, `swarm`'s one-thread-per-row design costs
  119 threads against `sweeper`'s 20 for close to the same CPU (mean 4.8%
  vs 3.9%) — the two designs differ in threads, not CPU.
- `warm` fast-forwards `lifecycle`'s 12-hour warm span in 1.65 s wall
  time, landing at 300 live sims and 49 MB RSS.
- Chaos verdicts: all six `contained` (`spin`, `throw`, `stale`, `flood`,
  `spawn-bomb`, `recurse`), unchanged from the earlier smoke run.

### Findings for 0.8

- `chaos/spin` pins a core near 100% CPU for about 8 s (samples t=4
  through t=11) before the operation budget ends it and `errored` reaches 1.
- `chaos/stale` does show `events/s` above `writes/s` at every sample
  (mean 59 vs 41): `FakerCtx::expire` broadcasts `Deleted` even when the id
  is already gone, so repeated deletes of a dead id emit events that are
  never counted as writes.
- `chaos/flood`'s 10k-insert burst ends normally (`errored: 0`), but the
  consumer falls badly behind: `lagged` reaches 8951 dropped events, peak
  lag hits ~19 s, and mean `events/s` (75) trails mean `writes/s` (671) by
  close to 9x while the burst is in flight.
