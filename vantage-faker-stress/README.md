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
cargo run -- ramp churn --steps 100,200,400,800 --hold 20s --dio --json /tmp/churn-ramp.json
jq '.[1]' baseline-0.7.json > /tmp/churn-0.7.json
cargo run -- compare /tmp/churn-0.7.json /tmp/churn-ramp.json
```

The last three lines rerun the baseline's `churn` ramp and compare it with
the recorded one. `baseline-0.7.json` holds six reports, so `jq` pulls out
`churn` (see Comparing versions).

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
| `--dio` | off | attaches a watching `Dio` with an open sorted scenery to every table, so CPU and RSS include the cache and re-list cost; lag still measures the counting subscriber, not the Dio |
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
after it. The column is the slowest table's figure, not a sum. The
subscriber only ever counts events. With `--dio` each table also backs a
`Dio` with a sorted scenery open, but the Dio follows the table through its
own native `watch`, not through this subscriber, so `events/s`, `lagged`
and `lag ms` still measure the counting subscriber alone. The Dio's apply
and re-list cost shows up in `cpu%` and `rss` instead.

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

- `seed`, `tables` and `sims` are vantage-faker's `DatasetSpec`
  (`vantage_faker::config`), as a vantage-ui faker datasource declares it.
  The tables are seeded into a `MemoryStore` before any sim starts.
- `tables.<name>` takes `count`, `columns`, `id_column`, `indexed`,
  `references` (column to target table) and `fan_out { column, min, max }`.
- `tables.<name>.columns.<col>` is `{ type, faker }`. `type` defaults to
  `string`. `faker` is a `ColumnGen` (`pick`, `range`, and the rest). A
  column with no generator, `{}`, falls back to `ValueGen`'s heuristics by
  column name and type.
- `!include <path>` reads a file relative to the scenario's own directory
  (here, `scenarios/churn/`) and can't resolve outside `scenarios/`.
- `sims.<name>` is a `SimSpec`: `table`, `script`, `clock`, `warm`, `ops`
  (Rhai operations allowed between two sleeps) and
  `spawn { burst, rate, max, args }`.
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
| `chaos/flood` | 10k inserts with no sleep | well under the operation budget, so it completes normally (`errored_min: 0`); watch `lagged`, `lag ms` and the Dio re-list cost |
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
- `churn`'s ramp runs all four steps without breaching either limit. Its
  top step (800) peaks at 299 live sims, since sims end as others spawn:
  mean CPU 29%, peak 78%, peak lag 20 ms.
- `swarm` stops after step 250 (peak lag 833 ms) and `sweeper` after
  step 100 (peak lag 397 ms). Both write their rows in bursts, and with
  `--dio` a burst of a few hundred events takes hundreds of milliseconds
  to drain; `lagged` stays 0. One sample decides the stop, so where a tick
  lands against a burst moves the stopping step. At step 100, the step
  both reach, `swarm`'s one-thread-per-row design costs 119 threads
  against `sweeper`'s 20 for the same mean CPU (8.0% each), so the two
  designs differ in threads, not CPU. Peak lag there is 127 ms for
  `swarm` and 397 ms for `sweeper`.
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
  consumer falls badly behind: `lagged` reaches 8956 dropped events, the
  backlog from the burst takes 3.2 s to get through (peak lag 3227 ms,
  skipped events included), and mean `events/s` (74) trails mean
  `writes/s` (672) by about 9x.

## Baseline (1.0)

Recorded in `baseline-1.0.json`, the same way as 0.7 (debug build, one
scenario at a time).

- `idle` is unchanged: 1000 parked sims at 100 MB RSS and 1018 threads.
- `swarm` and `sweeper` now both reach step 1000 (0.7 stopped at 250 and
  100). At 1000, `swarm` runs at 112% CPU, 131 MB, 1020 threads and
  `sweeper` at 107%, 36 MB, 21 threads: the same CPU, so the
  thread-per-sim design costs memory and threads, not CPU.
- `churn` is unchanged at its top step: 32% mean CPU (0.7: 29%) and 47
  writes/s in both.
- `warm` fast-forwards in 1.84 s (0.7: 1.65 s), landing at 300 live sims.
- `chaos/flood` now delivers every event: 671 events/s with 0 lagged
  (0.7: 74 events/s, 8956 lagged).
- `chaos/flood` is **not contained**, though: CPU is still 107% at the
  end. The Dio applies all 10k inserts one by one through its native
  watch, about 18 s pinned in a debug build, then around 15% steady at
  10k rows. That per-event Dio and scenery cost is worth batching.
- The other chaos scenarios (`spin`, `throw`, `stale`, `spawn-bomb`,
  `recurse`) are contained.
- The lag columns under `--dio` are not comparable with 0.7: 0.7 measured
  the Dio's apply, 1.0 measures only the counting subscriber.
