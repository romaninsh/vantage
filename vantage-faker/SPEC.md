# vantage-faker 1.0 on vantage-memory — design spec

Date: 2026-09-29 · Branch: `faker/memory` · Status: as built for 1.0.0, final-review rulings folded in

## Context

This is sub-project 2 of the faker rework:

0. Stress harness (done: `vantage-faker-stress`, baseline-0.7)
1. In-memory datasource (done: `vantage-memory` 0.6.0)
2. **vantage-faker 1.0 on vantage-memory** (this spec)
3. Port the remaining mutators to Rhai sims; migrate vantage-ui and the example apps
4. vantage-ui action kind that runs Rhai or spawns sims
5. Hospital sim game demo

vantage-faker 1.0.0 is not released yet. It is still in PR #407, which adds `SimEngine::stats`. This work goes into the same 1.0.0 release, and its CHANGELOG block grows to cover it.

Today faker keeps its rows in `MockShell` (a test mock) and pushes changes through its own broadcast channel. It has four live-data mechanisms (effects, `PulseSim`, `LiveFolderSim`, the sim engine), and relational tables that sims cannot write. After this change, faker does three things:

- **generates** tables,
- **seeds** them into a `vantage-memory` store,
- **runs Rhai sims** that mutate them.

The app reads and edits the same store through vantage-memory's Vista layer, and live updates come from its native `watch`.

## Decisions

- **Storage is vantage-memory only.** There is no pluggable storage and no cross-persistence (no SQLite or other backends).
- **A failed sim ends.** A sim that throws, exceeds a limit or panics inside a verb is counted as `errored` and stops. Nothing restarts it, and the rows it wrote stay. Idempotency means that scripts can write with `upsert` and stable ids, so a sim spawned twice with the same `args` doesn't duplicate data.
- **Linear scripts, one thread per sim.** No step engine. Both styles stay supported: one sim per row, and one sim looping over many rows.
- **Version 1.0.0** (unreleased), a breaking change from 0.7.x. Because 1.0 is the moment for breaking changes, the public API is settled now:
  - **Errors are `vantage_core::Result`** (`VantageError`), matching vantage-memory: `DatasetGen::generate`, `SimEngineBuilder::start`, `DatasetSpec::{generate, sim_defs, sim_builder, start_sims}` and `parse_duration`. The messages are the same text as before; callers that want a string use `.to_string()`.
  - **`SimStats` and the config structs are `#[non_exhaustive]` and `Default`**, so fields can be added later. Callers outside the crate build them from `Default` (or `DatasetSpec::new`) and assign fields.
  - **`FakerColumn.flags` is removed.** Nothing read it.

## What changes

| Keep | Change | Delete |
|---|---|---|
| `ColumnGen` generators, `ValueGen`, validation (`ColumnGen::validate`, `FanOut::validate`, `check_plan`) | Seeding writes into `MemoryTable`s through `DatasetGen` / `TableGen` | `FakerEffect`, `StaticEffect`, `FifoEffect` (`effect.rs`) |
| `relational_rows`, `FanOut`, `Reference`, `seed_id`, the tree generator | `FakerTable`, `FakerCtx` and `FakerHandle` are replaced by `MemoryStore` handles | `flights.rs` + `flights/*` |
| `geo` (the sim geo verbs use it) | The sim engine writes the store directly | `pulse.rs`, `live_folder.rs` + `live_folder/*` |
| `BackendShape` / `ShapedShell` (a test wrapper over any Vista) | The `rhai` feature is renamed to `sim` | `rhai_effect.rs` and its duplicate vocabulary |
| `SimEngine::stats` | `writes` comes from the store's write counters | the `fifo_cli`, `scenery_cli`, `scenery_folder_cli` and `live_folder_cli` examples, and the `live_folder` BDD test |

`examples/sims/flight.rhai` and `shipment.rhai` stay as example scripts and are updated to the verbs below.

## Features

- Default: generators and seeding. Dependencies are vantage-memory, vantage-types, vantage-vista (for `ShapedShell`), `fake`, `ciborium` and `indexmap`.
- `sim`: the Rhai sim engine. It adds `vantage-rhai`.
- `serde`: gates only the `config` module — the types in [Config types](#config-types). `serde` itself stays a normal dependency, and `ColumnGen` (and `ExtraFields`) deserialize in every feature set.

## Seeding

```rust
let store = MemoryStore::new();
DatasetGen::new(Some(42))
    .table(TableGen::new("client").column(FakerColumn::new("name", "string")).count(50))
    .table(
        TableGen::new("invoice")
            .column(FakerColumn::new("client_id", "string"))
            .reference("client_id", "client")
            .fan_out(FanOut { column: "client_id".into(), min: 1, max: 5 })
            .indexed(["client_id"]),
    )
    .generate(&store)?;
```

- `TableGen` holds a name, columns (`FakerColumn`, each with an optional `ColumnGen`), the id column (default `"id"`), `count`, references, an optional `fan_out`, `indexed` columns, and two row-content settings:
  - `weirdness(f)` — that fraction of string cells comes from the anomaly pool (`ValueGen::with_weirdness`);
  - `extra_fields(ExtraFields { count, size })` — `count` undeclared filler columns of `size` bytes, `extra_0001`…, added to every row after it is generated.
- `DatasetGen::generate(&store)` works as follows:
  1. **Validate everything first**, before any table is touched: a table name declared twice, every generator's `validate`, every `FanOut::validate`, a `fan_out` on a column that is not a reference, a table already in the store with a different id column, a reference to an unknown table, and a reference cycle. Each error names the table (and column).
  2. Order the tables so that referenced tables come before the tables that reference them.
  3. Generate every table's rows in memory, in that order. A reference column's `parent_count` is the number of rows its target **generated in this call**, never the store table's length, so a child always points at a row this call wrote (pre-existing parent rows are never picked). `check_plan` runs here, so a `fan_out` over a parent that generated no rows fails before any table is written.
  4. Define each table in the store with its id column, call `add_index` for every `indexed` column (so a table that already existed gets its indexes too), and seed it quietly — one `Reset` when it is unquieted.
  5. Return the table handles in generation order.
- Every table is an ordinary `MemoryTable`, relational or not, so sims and the app can write any of them.
- The same seed gives the same rows. With `None`, values come from entropy.
- Seeded row ids come from `seed_id(seq)`, a zero-padded 20-digit string, as before. `relational_rows` assumes parent ids are `seed_id(0..parent_count)`. A table with `count: 0` is created empty.

## Sim engine (`sim` feature)

### Builder

```rust
let engine = SimEngine::builder()
    .store(&store)
    .sim(SimDef::new("parcel", "shipment", script).with_spawn(40, 2.5, 120).with_clock(60.0))
    .seed(7)
    .start()?;            // runs the warm start, then goes live
```

- Scripts reach tables by name in the store. The builder no longer registers tables one by one.
- A def's default `table` must exist when `start()` runs. Otherwise `start()` returns an error.
- A script naming a table that doesn't exist fails with a script error, which ends that sim.
- The engine keeps the store alive; the old "tables dropped → stop" check is gone. Dropping the engine stops the sims and joins their threads.
- `start()` returns `vantage_core::Result<SimEngine>`.

### Verbs

Script-facing names and signatures are unchanged unless listed here. `t?` means an optional table name that defaults to the def's table.

| Verb | Behaviour |
|---|---|
| `insert(t?, map) -> id` | `MemoryTable::insert`. The map is stored as given, plus the id — there is no declared-column shaping, since a `MemoryTable` has no column list (missing fields read as null). If the map carries the id column, that id is used, and an existing row with it is a script error. |
| `upsert(t?, id, map)` **new** | `MemoryTable::upsert`. This is the idempotent write for stable ids, such as `"parcel-" + sim_id()`. |
| `patch(t?, id, map)`, `set(t?, id, field, v)`, `delete(t?, id)` | Store `patch` / `delete`. A missing id is a silent no-op, as today. |
| `get(t?, id) -> map \| ()`, `ids(t?) -> [id]`, `count(t?) -> int` | Store reads. |
| `find(t?, #{col: val, …}) -> [id]` **new** | Ids of rows matching every `col == val` (a `Query` of `Eq` conditions, so it uses indexes). Rows come back in insertion order. |
| random, time, spawn, geo verbs | Unchanged. |

Row ids are strings throughout: verbs return them as strings and take them as strings.

### Warm start and quiet

- Warm start sets every store table quiet, runs the warm span, then unquiets every table. A caller's own quiet state is not preserved: every table comes out unquieted.
- vantage-memory then sends one `Reset` per table that was written while quiet, and watchers (Vista `watch`, and Dio through it) re-list.
- The engine no longer tracks quiet itself.
- **Panic guard.** A drop guard owns the quiet span. If `start()` unwinds during the warm start (for example a panicking `on_warm_progress` callback), the guard stops the scheduler, joins every sim thread and unquiets every table before the panic propagates, so a failed start leaves no sim running and no table muted.

### The `set_quiet` race (vantage-memory 0.6.1)

- **The problem.** `MemoryTable::set_quiet` sets the `quiet` and `missed` flags without holding the table's write lock. A write that is running at the same moment can drop its event and leave `missed` set, which later produces a spurious `Reset`.
- **The fix.** `set_quiet` takes the rows write lock, so a quiet change never interleaves with a write.
- A concurrent test drives writes and quiet toggles, and checks that every write either reached a subscriber or was followed by a `Reset`.
- vantage-memory goes to 0.6.1, with a CHANGELOG line.

### Isolation

- **Operations budget.** A sim may spend at most `ops` Rhai operations between two sleeps before it is ended as a runaway. The default drops from 50M to **5M**. A `SimDef::with_ops(n)` (YAML `ops:`) overrides it per def.
- **Other limits are unchanged:**
  - `MAX_LIVE` = 1000 live sims per engine
  - call-level and expression-depth limits
  - `MAX_STILL_SLEEPS`
  - panic containment in `Release::drop`
- **Stats.** `SimEngine::stats()` keeps its fields; `SimStats` is `#[non_exhaustive]`.
  - `writes` counts sim verb calls:
    - `insert` always counts (when it succeeds);
    - `upsert` counts when it returns `Inserted` or `Updated` (not `Unchanged`);
    - `patch`, `set` and `delete` count when the row existed, even if the values were unchanged.
  - Seeding and the app's own writes are not counted.

## Config types

These are the faker part of a datasource definition, deserialized from YAML with the `serde` feature. vantage-ui, the stress harness and the examples share them.

Every struct below is `#[non_exhaustive]` and `Default`; `DatasetSpec::new(seed, tables, sims)` builds the top level.

```rust
pub struct DatasetSpec {
    pub seed: Option<u64>,
    pub tables: IndexMap<String, TableSpec>,
    pub sims: IndexMap<String, SimSpec>,       // needs the `sim` feature to run
}
pub struct TableSpec {
    pub id_column: Option<String>,             // default "id"
    pub count: usize,                          // default 0
    pub columns: IndexMap<String, ColumnSpec>, // ColumnSpec { type: Option<String>, faker: Option<ColumnGen> }
    pub indexed: Vec<String>,
    pub references: IndexMap<String, String>,  // column → target table
    pub fan_out: Option<FanOutSpec>,           // { column, min, max }
    pub weirdness: Option<f64>,                // TableGen::weirdness, default 0
    pub extra_fields: Option<ExtraFields>,     // { count, size }
}
pub struct SimSpec {
    pub table: Option<String>,
    pub script: String,
    pub clock: Option<f64>,
    pub warm: Option<String>,                  // "90m", "6h", "3d"
    pub ops: Option<u64>,
    pub spawn: SpawnSpec,                      // { burst, rate, max, args }
}
```

- `DatasetSpec::generate(&store) -> Result<()>` seeds the tables.
- `DatasetSpec::sim_builder(&store) -> Result<Option<SimEngineBuilder>>` (`sim` feature) returns a builder with every def added and the dataset's `seed` applied, so a caller can set a manual clock or a warm-progress callback before starting. `None` when `sims` is empty.
- `DatasetSpec::start_sims(&store) -> Result<Option<SimEngine>>` is shorthand for `sim_builder` then `start`.
- All of these return `vantage_core::Result`.
- **Defaults** follow vantage-ui's: `burst: 1`, `rate: 0`, `max: 1`, `clock: 1`. A sim with no `table` uses the first table.
- **Parsing.** Durations are parsed as vantage-ui parses them. `deny_unknown_fields` is set on every struct.
- **`!include`** resolution stays with each caller. vantage-ui and the harness already have their own loaders.
- The stress harness replaces its private copy with these types.

## `BackendShape`

`BackendShape` describes transport only: the advertised capabilities, the page size, the latency model, the fault schedule and a `seed`. The seed makes latency jitter, fault draws and boundary skew replay; it does not affect row values. Row content — `weirdness` and `extra_fields` — moved to `TableGen` / `TableSpec`, because it is generated data, not transport, and a shape field nothing applied was a trap. `ExtraFields` lives next to `TableGen`.

## Testing

- **Carried over and ported:**
  - the generator, relational, tree and validation tests
  - the sim engine tests (manual clock, spawner, warm start, verbs, limits, stats), rewritten against the store
- **New:**
  - `DatasetGen`: reference ordering, a cycle error, the same rows for the same seed, indexes defined, `count: 0`, and an existing-table id mismatch error
  - sims writing to a relational (fan-out) table
  - `upsert`: idempotent across two spawns with the same args and stable id
  - `find`: equality, indexed and unindexed, insertion order
  - warm start ends with a `Reset` on each table written while warm
  - `ops`: a spinning sim with `ops: 100_000` ends quickly and counts as errored
  - `DatasetSpec` YAML round-trip, defaults, and `deny_unknown_fields` errors
  - the `set_quiet` concurrency test in vantage-memory
- **Added at final review:**
  - references pick only rows generated in the same call; validation errors leave the store untouched; duplicate table names; `add_index` on an existing table
  - a sim writing a fan-out table, and inserting and `find`ing in a table keyed by `code`
  - `find` on an indexed column
  - the ops budget is per def: the same ~300k-op loop succeeds on the default and errors with `ops: 100_000`
  - a panicking warm-progress callback leaves no sim thread and no quiet table
- **Harness:**
  - Move `vantage-faker-stress` onto the 1.0 API: `DatasetSpec` + store, and the lag probe subscribes via each table's store channel.
  - Re-run the load and chaos scenarios, save `baseline-1.0.json`, and compare it with 0.7 in its README.
  - Expected: `spin` ends in about 1 s in debug, and `stale` shows no phantom events.

## Out of scope

- Porting fifo, flights, pulse and live_folder to Rhai, and migrating vantage-ui and the examples (sub-project 3)
- Restarting or cleaning up failed sims
- Pluggable or cross-persistence storage
- Changes to the generators themselves
- Deferred to sub-project 3: an atomic claim verb (`patch_if`), range and limit in `find`, `find` returning row maps, and a reference to a 0-row table without `fan_out` (it keeps the column's own generated values)
