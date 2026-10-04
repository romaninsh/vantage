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
  CHANGELOG.md
  SPEC.md                    this file
  src/lib.rs                 re-exports; prelude
  src/store.rs               MemoryStore: table registry, sync core API
  src/store/table.rs         one table: rows+indexes under one lock, id gen, event channel, quiet flag
  src/store/ids.rs           id generation
  src/store/index.rs         hash indexes per declared column
  src/store/events.rs        MemoryChange, change broadcast
  src/store/query.rs         MemoryTable::query / count: index candidates or full scan
  src/store/write.rs         MemoryTable writes (insert / insert_as / replace / upsert / patch / delete), UpsertOutcome
  src/eval.rs                Query and its evaluation pipeline
  src/eval/condition.rs      MemoryCondition matching
  src/eval/compare.rs        cell comparison and dotted paths
  src/eval/order.rs          multi-key ordering
  src/types.rs               AnyMemoryType (vantage_type_system! over CBOR)
  src/types/{bool,bytes,numbers,string,value}.rs   per-variant conversions
  src/typed.rs               MemoryDB: DataSource / ExprDataSource
  src/typed/operation.rs     MemoryOperation: column → MemoryCondition, .ascending()/.descending()
  src/typed/convert.rs       record ⇄ AnyMemoryType; table conditions/orders/pagination → Query
  src/typed/table_source.rs  TableSource for MemoryDB
  src/typed/aggregate.rs     sum / max / min
  src/vista.rs               MemoryTableShell: query state over one store table
  src/vista/shell.rs         TableShell impl
  src/vista/refs.rs          Catalog; get_ref / get_ref_target
  src/vista/watch.rs         watch_vista
  src/vista/writes.rs        insert / replace / patch / delete / import
  src/vista/factory.rs       MemoryVistaFactory + MemoryVistaSpec
  src/seed.rs                load / load_file / dump
  examples/bench.rs          throughput at 1k / 10k / 100k rows
  tests/                     typed, vista, watch, dio, seed_joins
```

Dependencies: vantage-core, vantage-types, vantage-expressions, vantage-dataset, vantage-table, vantage-vista (all path + version, like the other workspace crates), `ciborium`, `indexmap`, `parking_lot`, `tokio` (`sync`), `async-trait`, `serde`, `serde_json`. Dev-dependencies: vantage-diorama, `tokio` (`full`), `serde_yaml_ng`.

## Store

`MemoryStore` is a cheap-to-clone handle (`Arc`) over a registry of named tables. Each table is:

```
MemoryTable {
    rows:    parking_lot::RwLock<Rows>,  // rows and their indexes share one lock
    id:      IdGen,                      // per-table counter + optional prefix
    events:  broadcast::Sender<MemoryChange>,
    quiet:   AtomicBool,
    writes:  AtomicU64,
}

struct Rows {
    map:     IndexMap<String, Arc<Record<CborValue>>>,  // insertion order kept
    indexes: Indexes,                                    // declared columns only; maintained on every write
}
```

**One lock per table.** A table's rows and its indexes are behind the same `RwLock`, not two separate ones, so an index can never be read out of step with the rows it was built from.

**The core API is synchronous.** Faker sims run on OS threads and call it directly.

- `table(name) -> MemoryTableHandle`. It creates the table if it's missing. `define(name, TableDef { id_column, indexed, id_prefix })` sets a table up explicitly.
- On a table handle:
  - `insert(record) -> id`. It generates the id, or uses the id column when present. An id that already exists is an error.
  - `insert_as(id, record) -> Result<Row>` inserts under a given id; an id that already exists is an error.
  - `replace(id, record) -> Option<Row>` replaces an existing row; `None`, with nothing written or sent, when it is missing.
  - `upsert(id, record) -> UpsertOutcome` (`Inserted`, `Updated` or `Unchanged`) inserts or replaces. It is idempotent, which is what sims use.
  - `patch(id, partial) -> bool` merges fields, ignoring any value for the id column. It returns `false` when the row is missing, and in that case sends no event.
  - `delete(id) -> bool` returns `false` when the row is missing, and in that case sends no event. It is O(n): the row map shifts later rows down to keep insertion order.
  - Each write checks, writes, maintains the indexes and sends its event under one hold of the write lock, so the typed and Vista write paths built on them cannot race between the check and the write.
  - `get(id) -> Option<Arc<Record>>`, `ids()`, `len()`.
  - `query(&Query) -> Vec<(String, Arc<Record>)>` and `count(&Query) -> usize`.
  - `set_quiet(bool)` and `writes()`. Leaving quiet mode after at least one write was made while quiet sends `MemoryChange::Reset`.
  - `add_index(column)` adds a hash index over every existing row; a no-op if the column is already indexed. `is_indexed(column) -> bool`.
- **Events.** Every write that changes a row sends one `MemoryChange` on the table's channel, unless the table is quiet: `Inserted { id, row }`, `Updated { id, row, old }` or `Deleted { id, old }` — no `Option` fields; each variant only carries what it has. `Reset` carries nothing and tells subscribers to re-list. The enum is `#[non_exhaustive]`; `id()` returns `Option<&str>` (`None` for `Reset`). Writes that change nothing send nothing. Missing ids are the clearest case, and this fixes a finding from the 0.7 baseline, where a delete of a missing id still broadcast. The channel capacity is 4096. A subscriber that lags gets `Lagged` and re-lists, because the store is the source of truth. The send happens while the write still holds the table's lock, so a watcher that reacts by re-reading the table through a fresh lock is guaranteed to see at least that write already applied.
- **Ids.** Ids are strings. Generated ids are a per-table counter, `"1"`, `"2"`… or `"<prefix>1"`. A caller-supplied id is used as given.

## Evaluation

The typed layer and the Vista layer both compile down to one `Query`:

```rust
pub struct Query {
    pub conditions: Vec<MemoryCondition>,           // AND-ed
    pub search: Option<String>,
    pub order: Vec<(String, vantage_vista::SortDirection)>,  // multi-key, stable
    pub offset: usize,
    pub limit: Option<usize>,
}

pub enum MemoryCondition {
    Cmp { path: String, op: FilterOp, value: CborValue }, // Eq Ne Gt Gte Lt Lte InSet NotInSet Like
    Search(String),
    And(Vec<MemoryCondition>),
    Or(Vec<MemoryCondition>),
    Not(Box<MemoryCondition>),
    Column(String),                          // bare column reference, valid only as an order key
    Deferred(DeferredFn<AnyMemoryType>),     // references; resolved to the above before evaluation
}
```

`Column` is not a filter: evaluating it with `matches()` is an error. It exists so an order key and a filter condition share one type — see Typed layer below.

The pipeline runs in this order: resolve `Deferred` → choose the candidate rows → match the conditions → apply search → order → offset/limit.

**Candidate rows.** If an `Eq` or `InSet` condition on an indexed column is AND-ed at the top level, its index supplies the candidate ids. Otherwise every row is a candidate. The index keys an integer, or a float with no fractional part, within ±2^53 as an exact number, so `1` and `1.0` land in the same bucket; anything else (larger integers, non-integral floats) keys by its `f64` bit pattern. This can put two unequal huge integers in the same bucket — `matches_all` still filters the candidates — but never splits two values that `values_eq` would call equal.

Semantics:

- **Paths.** A path is a column name or a dotted path into nested maps (`address.city`). A missing path reads as null.
- **Comparison.** Integers and floats compare numerically across the two types. Text compares by code point. Booleans compare false < true. Values of different kinds don't match `Gt`/`Gte`/`Lt`/`Lte`.
- **Null.** A null cell matches `Eq null`, `Ne <non-null value>`, and `NotInSet` when the set holds no null. It fails every other op.
- **`Like`.** `%` and `_` wildcards, case-insensitive, over the text form of the cell. `FilterOp`'s existing `operand_text` supplies the pattern text.
- **Search.** Case-insensitive substring over every text cell, and over the text form of numbers.
- **Ordering.** `Query.order` carries `vantage_vista::SortDirection`. Nulls sort first ascending and last descending — descending reverses the whole comparison, nulls included, rather than only the non-null values. NaN ranks after every other number, so the order is total. Ties keep insertion order.

The evaluator is a public module. Diorama's and MockShell's copies stay as they are; moving them onto this one is later cleanup.

## Typed layer

`MemoryDB` wraps a `MemoryStore` and implements `DataSource`, `ExprDataSource` and `TableSource`:

- `Value = AnyMemoryType`. This is a type-system wrapper over CBOR, following the same pattern as `AnyRedbType` and `AnyCsvType`. `Id = String` and `Condition = MemoryCondition`.
- The `MemoryOperation` trait on columns has `.eq() .ne() .gt() .gte() .lt() .lte() .in_() .not_in() .like()`, and each builds a `MemoryCondition`. `eq_condition` and `eq_value_condition` build `Cmp Eq`. It also has `.ascending()` / `.descending()`, each building `OrderBy<MemoryCondition>` over `MemoryCondition::Column(name)`.
- Building a `Query` from a `Table`: its conditions are AND-ed together in the order added; each of `Table::orders()`'s keys must be a `MemoryCondition::Column` (the only thing `.ascending()`/`.descending()` produce) or building the query errors; `vantage_table::sorting::SortDirection` maps onto `vantage_vista::SortDirection` one-to-one; a set `Pagination`'s skip and limit are clamped to 0 if negative before becoming the query's offset/limit.
- Implemented: list, get, get_some, count, sum, min, max (over matching rows; non-numeric cells are skipped for sum), insert, insert_return_id, replace, patch, delete, delete_all, pagination, ordering and references (via `Deferred`). Invariants (uniqueness, required fields, etc.) are enforced by vantage-table's shared set layer before a write reaches `MemoryDB`, not by this crate, as for other backends.
- Errors: writing to a missing id returns an error, as the typed contract expects. The store's `bool` returns are for the sync API only. A typed table whose id column differs from an existing store table's is an error on every operation, as in the factory.

## Vista layer

`MemoryTableShell` implements `TableShell` directly over the store — a `MemoryTableHandle` plus its own `Query` — not over a typed `Table`. Each handle shares the table and keeps its own query state: op-conditions, orders, search and page size. `clone_shell` gives fresh query state over the same table.

- **Capabilities set:**
  - count, insert, update, delete, import (`import_vista_values` upserts every given record and returns how many ids were new, not the total processed)
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

  Telling these cases apart needs the row as it was before the write, so `MemoryChange::Updated` carries `old` alongside `row`, and `MemoryChange::Deleted` carries `old` in place of `row`.
  - a `Lagged` channel, or a `MemoryChange::Reset`, produces `Invalidated`

  Rows never trigger a re-query.
- **Traversal (`get_ref` / `get_ref_target`).** `HasOne`: the target Vista is narrowed to `target[id] == row[foreign_key]`, the foreign key taken as an id string (an integer `1` becomes `"1"`). `HasMany`: narrowed to `target[foreign_key] == row[id]` — the SQLite backend's convention — matching the id as text or, when it parses as an integer, as an integer too (`InSet`), since seed files often write foreign keys as integers. The typed layer's `related_in_condition` widens its source values the same way. A row missing the join field (its own id column for `HasMany`, or the foreign key for `HasOne`) is an error, not an empty result.
- **The reference catalog.** The factory keeps a `Catalog`: a shared, `Clone`-cheap map from every vista name it has built to that vista's `VistaMetadata`. `get_ref`/`get_ref_target` build the target shell's metadata from the catalog, so a traversal target gets its declared columns and references. A target table that was never built through this factory (or not yet) gets id-only metadata instead of an error.
- **`MemoryVistaFactory`** implements `VistaFactory` and is built from a `MemoryStore` handle. `MemoryVistaSpec` (`VistaSpec<MemoryTableExtras, NoExtras, NoExtras>`) carries the table name, columns, id column, references and the driver extras `MemoryTableExtras { memory: MemoryBlock }`, where `MemoryBlock { indexed: Vec<String>, seed: Option<PathBuf> }` — YAML `memory: { indexed: [..], seed: <file>? }`.
  - `indexed` columns are applied to the table with `add_index`, including a table that already existed under another spec; only the id column has to agree — a mismatch between the spec's `id_column` and an existing table's is an error, since the table can't be given a second id column after rows exist.
  - `seed`, when set, is loaded into the table via `seed::load_file` when the vista is built and the table is empty, so rebuilding a vista neither reverts edits nor duplicates rows. A relative path resolves against the process working directory.
  - `lazy:` columns become Vista computed columns (needs the `rhai` feature).
  - Rejected: `contained:` relations, multi-key `references:` (non-empty `keys:`), and columns with `expr:` — all return an error naming the offending vista/column/reference.
  - A column's `references:` accepts the shorthand `references: <target_table>` (a `HasOne` keyed on the column itself) alongside the full form `{ table, kind, foreign_key }`; a table-level `references:` entry needs the same `table:`/`kind:`/`foreign_key:` shape. `foreign_key` defaults to the column or reference name when omitted.
  - Every stored column gets the `orderable` flag, whether or not the spec set it, since the store can sort on any field. A `lazy:` column is computed by the Vista and is not orderable.
  - The id column is `spec.id_column` if set, else the column carrying the `id` flag, else `"id"`.
  - The vantage-ui kind that uses these specs comes in sub-project 3.

## Seed and dump

`seed::load(table, rows: impl IntoIterator<Item = serde_json::Value>)` upserts objects by their id column, inserting when the object has none; this is quiet when the table is quiet. Rows convert to and from stored CBOR via vantage-types' `Record`⇄CBOR conversions (`CborValue::serialized`/`.deserialized()`), the same path the typed layer uses. `seed::load_file(table, path)` reads a `.yaml`/`.yml` file as YAML; every other extension (including none) is read as JSON. `seed::dump(table) -> Result<Vec<serde_json::Value>>` returns rows in insertion order; a row with a cell JSON cannot express (bytes, tags, non-text map keys) is an error.

## Performance

- The lock is per table, never global, so writers on different tables don't contend. A query holds the read lock while it picks candidates and evaluates the conditions and search on them; ordering and windowing run after it is released.
- Rows are `Arc<Record>`: the sync store API and the event fan-out share rows instead of deep-copying them; the typed and Vista layers return owned records, cloned from those rows. `patch` builds a new `Arc` for the changed row only.
- Hash indexes on declared columns turn `Eq`/`InSet` lookups and reference traversal into lookups instead of scans. Index buckets are unordered (removal swaps); candidates are put back in insertion order by their position in the row map. A write that leaves every indexed cell unchanged skips index maintenance.
- Watch matching costs O(conditions) per change per subscriber.
- `examples/bench.rs` prints ops/s and µs/op for insert, patch, get, an indexed `Eq` query on a low-cardinality and on a selective column, an unindexed range query, an ordered page, and delete, at 1k, 10k and 100k rows. It prints numbers; it never asserts them. A release run's numbers are in the README. The relative figures that matter come from `vantage-faker-stress` once faker moves onto this store in sub-project 2.

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
