# vantage-memory — design spec

Date: 2026-09-29 · Branch: `memory/datasource` · Status: draft for review

## Context

This is sub-project 1 of the faker rework:

0. Stress harness (done: `vantage-faker-stress`)
1. **In-memory datasource** (this spec)
2. vantage-faker 0.8: pluggable storage, a single build path, write events, the `sim` feature, idempotent writes, sim isolation
3. Port the mutators to Rhai sims; migrate vantage-ui and the example apps (this includes the vantage-ui `type: memory` kind)
4. vantage-ui action kind that runs Rhai or spawns sims
5. Hospital sim game demo

Vantage has four in-memory stores today, and none of them is a real datasource:

- `ImDataSource`/`ImTable` (vantage-dataset) covers the dataset traits only.
- `MockShell` (vantage-vista) supports only `==` filters, has no `watch`, and is faker's production store.
- `MockTableSource` (vantage-table) ignores conditions altogether.
- diorama's `MemoryCache` is a Lens cache, not a datasource.

Condition evaluation is also written four or five times in different crates.

`vantage-memory` is a real in-memory datasource. It becomes faker's storage in sub-project 2. Rhai sims and the app both read and write it, and its change events drive Dio and scenery, so it also puts real load on those layers.

## Decisions

- **Both layers.** It has a typed `TableSource` (`Table<MemoryDB, E>`) and a Vista `TableShell` + `VistaFactory`, like vantage-sql. Both layers read one store through one evaluator.
- **Pure memory.** Data lives as long as the process. Seeding and dumping are public functions, so a snapshot file can be added later without redesign.
- **Mocks stay test-only.** `MockShell`, `MockTableSource` and `ImDataSource` are kept as they are, for tests. Non-test code must not use mocks. `ImTable` keeps its one production role as the store behind `ContainedShell`, the Vista for embedded collections.
- **Our own store, designed for speed.** SQL engines (SQLite in-memory) and SurrealDB were considered and rejected: SQL sits in the write path, and SurrealDB is a very heavy dependency.
- **Version 0.6.0.** The crate is a workspace member, and the version signals that the design isn't final.

## Crate layout

`vantage-memory/` is added to the root workspace `members`. No file should grow past about 200 LOC. There are no `mod.rs` files.

```
vantage-memory/
  Cargo.toml                 version 0.6.0
  README.md
  SPEC.md                    this file
  src/lib.rs                 re-exports; MemoryDB
  src/store.rs               MemoryStore: table registry, sync core API
  src/store/table.rs         one table: rows, per-table lock, indexes, event channel, quiet flag
  src/store/ids.rs           id generation
  src/store/index.rs         optional hash indexes per declared column
  src/store/events.rs        change broadcast
  src/eval.rs                Query and its evaluation pipeline
  src/eval/condition.rs      MemoryCondition matching
  src/eval/compare.rs        cell comparison and dotted paths
  src/eval/order.rs          multi-key ordering
  src/typed.rs               TableSource for MemoryDB
  src/typed/operation.rs     MemoryOperation: column → MemoryCondition
  src/typed/type_system.rs   AnyMemoryType
  src/typed/aggregate.rs     count / sum / min / max
  src/vista.rs               MemoryTableShell: TableShell
  src/vista/watch.rs         watch_vista
  src/vista/factory.rs       MemoryVistaFactory + MemoryTableSpec
  src/seed.rs                load / dump rows as serde values
  examples/bench.rs          throughput at 1k / 10k / 100k rows
  tests/                     typed, vista, watch, dio, seed
```

Dependencies: vantage-core, vantage-types, vantage-expressions, vantage-dataset, vantage-table, vantage-vista (all path + version, like the other workspace crates), `ciborium`, `indexmap`, `parking_lot`, `tokio` (`sync`), `async-trait`, `serde`, `serde_json`. Dev-dependencies: vantage-diorama, `tokio` (`full`), `serde_yaml_ng`.

## Store

`MemoryStore` is a cheap-to-clone handle (`Arc`) over a registry of named tables. Each table is:

```
MemoryTable {
    rows:    parking_lot::RwLock<IndexMap<String, Arc<Record<CborValue>>>>,  // insertion order kept
    id:      IdGen,                     // per-table counter + optional prefix
    indexes: HashMap<String, HashIndex>,// declared columns only; maintained on every write
    events:  broadcast::Sender<MemoryChange>,
    quiet:   AtomicBool,
    writes:  AtomicU64,
}
```

**The core API is synchronous.** Faker sims run on OS threads and call it directly.

- `table(name) -> MemoryTableHandle`. It creates the table if it's missing. `define(name, TableDef { id_column, indexed, id_prefix })` sets a table up explicitly.
- On a table handle:
  - `insert(record) -> id`. It generates the id, or uses the id column when present. An id that already exists is an error.
  - `upsert(id, record)` inserts or replaces. It is idempotent, which is what sims use.
  - `patch(id, partial) -> bool` merges fields. It returns `false` when the row is missing, and in that case sends no event.
  - `delete(id) -> bool` returns `false` when the row is missing, and in that case sends no event.
  - `get(id) -> Option<Arc<Record>>`, `ids()`, `len()`.
  - `query(&Query) -> Vec<(String, Arc<Record>)>` and `count(&Query) -> usize`.
  - `set_quiet(bool)` and `writes()`.
- **Events.** Every write that changes a row sends one `MemoryChange::{Inserted, Updated, Deleted}{ id, row: Option<Arc<Record>>, old: Option<Arc<Record>> }` on the table's channel, unless the table is quiet. Writes that change nothing send nothing. Missing ids are the clearest case, and this fixes a finding from the 0.7 baseline, where a delete of a missing id still broadcast. The channel capacity is 4096. A subscriber that lags gets `Lagged` and re-lists, because the store is the source of truth.
- **Ids.** Ids are strings. Generated ids are a per-table counter, `"1"`, `"2"`… or `"<prefix>1"`. A caller-supplied id is used as given.

## Evaluation

The typed layer and the Vista layer both compile down to one `Query`:

```rust
pub struct Query {
    pub conditions: Vec<MemoryCondition>,    // AND-ed
    pub search: Option<String>,
    pub order: Vec<(String, SortDir)>,       // multi-key, stable
    pub offset: usize,
    pub limit: Option<usize>,
}

pub enum MemoryCondition {
    Cmp { path: String, op: FilterOp, value: CborValue }, // Eq Ne Gt Gte Lt Lte InSet NotInSet Like
    Search(String),
    And(Vec<MemoryCondition>),
    Or(Vec<MemoryCondition>),
    Not(Box<MemoryCondition>),
    Deferred(DeferredFn<AnyMemoryType>),     // references; resolved to the above before evaluation
}
```

The pipeline runs in this order: resolve `Deferred` → choose the candidate rows → match the conditions → apply search → order → offset/limit.

**Candidate rows.** If an `Eq` or `InSet` condition on an indexed column is AND-ed at the top level, its index supplies the candidate ids. Otherwise every row is a candidate.

Semantics:

- **Paths.** A path is a column name or a dotted path into nested maps (`address.city`). A missing path reads as null.
- **Comparison.** Integers and floats compare numerically across the two types. Text compares by code point. Booleans compare false < true. Values of different kinds don't match `Gt`/`Gte`/`Lt`/`Lte`.
- **Null.** A null cell matches `Eq null`, `Ne <non-null value>`, and `NotInSet` when the set holds no null. It fails every other op.
- **`Like`.** `%` and `_` wildcards, case-insensitive, over the text form of the cell. `FilterOp`'s existing `operand_text` supplies the pattern text.
- **Search.** Case-insensitive substring over every text cell, and over the text form of numbers.
- **Ordering.** Null sorts first. Ties keep insertion order.

The evaluator is a public module. Diorama's and MockShell's copies stay as they are; moving them onto this one is later cleanup.

## Typed layer

`MemoryDB` wraps a `MemoryStore` and implements `DataSource`, `ExprDataSource` and `TableSource`:

- `Value = AnyMemoryType`. This is a type-system wrapper over CBOR, following the same pattern as `AnyRedbType` and `AnyCsvType`. `Id = String` and `Condition = MemoryCondition`.
- The `MemoryOperation` trait on columns has `.eq() .ne() .gt() .gte() .lt() .lte() .in_() .not_in() .like()`, and each builds a `MemoryCondition`. `eq_condition` and `eq_value_condition` build `Cmp Eq`.
- Implemented: list, get, get_some, count, sum, min, max (over matching rows; non-numeric cells are skipped for sum), insert, insert_return_id, replace, patch, delete, delete_all, pagination, ordering and references (via `Deferred`). Invariants are enforced through vantage-table's shared path, as for other backends.
- Errors: writing to a missing id returns an error, as the typed contract expects. The store's `bool` returns are for the sync API only.

## Vista layer

`MemoryTableShell` implements `TableShell`. Each handle shares the store and keeps its own query state: op-conditions, orders, search and page size. `clone_shell` gives fresh query state over the same store.

- **Capabilities set:**
  - count, insert, update, delete, import
  - order, search, filter operators
  - set page size, fetch page, fetch window
  - traverse to record, traverse to set
  - subscribe
- **Not set:** `can_invalidate`, since nothing outside the process changes the data. `aggregate_vista` is not implemented; it keeps its default, like every other driver.
- **`watch_vista`** subscribes to the table's channel. It checks each change against the Vista's conditions once, on the changed row only:
  - a row that newly matches arrives as `Inserted`
  - a row that still matches arrives as `Updated`
  - a row that stopped matching, or was deleted while matching, arrives as `Deleted`
  - a change to a row that matches neither before nor after is skipped

  Telling these cases apart needs the row as it was before the write, so `MemoryChange` also carries `old: Option<Arc<Record>>`.
  - a `Lagged` channel produces `Invalidated`

  Rows never trigger a re-query.
- **`MemoryVistaFactory`** implements `VistaFactory` and is built from a `MemoryStore` handle. `MemoryTableSpec` carries the table name, columns, id column, `indexed` columns and references. The YAML extras are `memory: { indexed: [..], seed: <file>? }`. The vantage-ui kind that uses them comes in sub-project 3.

## Seed and dump

`seed::load(table, rows: impl IntoIterator<Item = serde_json::Value>)` upserts objects by their id column; this is quiet when the table is quiet. `seed::load_file(table, path)` reads a JSON or YAML array. `seed::dump(table) -> Vec<serde_json::Value>` returns rows in insertion order.

## Performance

- The lock is per table, never global, so writers on different tables don't contend. Reads take a read lock only for as long as it takes to collect the candidate `Arc`s.
- Rows are `Arc<Record>`: listing, events and watch share rows instead of deep-copying them. `patch` builds a new `Arc` for the changed row only.
- Hash indexes on declared columns turn `Eq`/`InSet` lookups and reference traversal into lookups instead of scans.
- Watch matching costs O(conditions) per change per subscriber.
- `examples/bench.rs` prints ops/s for insert, patch, get, an indexed `Eq` query, an unindexed range query, and an ordered page, at 1k, 10k and 100k rows. It prints numbers; it never asserts them. The relative figures that matter come from `vantage-faker-stress` once faker moves onto this store in sub-project 2.

## Testing

1. **`eval` unit tests:**
   - every `FilterOp`, including across int and float
   - dotted and missing paths
   - null rules
   - `Like` wildcards
   - And/Or/Not
   - search
   - multi-key stable ordering, and offset/limit
   - the indexed candidate path agreeing with a full scan
2. **Store tests:**
   - id generation, including a supplied id
   - duplicate insert is an error
   - `upsert` is idempotent
   - `patch`/`delete` on a missing id return `false` and send no event
   - quiet suppresses events but still stores the write
   - `writes` counts only changing writes
   - indexes stay correct across patch and delete
3. **Typed-layer tests:** conditions built with column operations, list/get/count/sum/min/max under conditions, CRUD, pagination, ordering, references, invariants on write.
4. **Vista-layer tests:** the advertised capabilities hold, op-conditions/order/search/page/window work, and `clone_shell` isolates query state.
5. **Watch tests:**
   - changes made through the typed layer, the Vista layer and the sync store API all reach `watch_vista`
   - a filtered Vista sees a row leave as `Deleted`
   - lag produces `Invalidated`
6. **Dio test:** a Dio over a memory Vista, with a sorted TableScenery open, stays correct through inserts, patches and deletes. It updates through `watch()` alone, with no `handle_event` forwarding.
7. **Seed tests:** load and dump round-trip, and YAML and JSON files.

## Out of scope

- Snapshot or journal persistence
- Migrating the mocks or `ContainedShell` onto this store
- Moving diorama's or MockShell's evaluators onto this one
- The vantage-ui `type: memory` kind, and faker's switch-over (sub-projects 3 and 2)
- Transactions spanning several writes
