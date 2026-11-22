# Changelog

## 5.0.0 — 2026-11-22

- **Breaking:** `insert_table_value` returns `Inserted<AnyMemoryType>` and the vista shell's
  `insert` returns `(String, Inserted<CborValue>)`. Use `.into_record()` for the row.
- An insert reports `Created`, or `Existed` with the stored row when the id is already in the
  set.
- Requires vantage-dataset 2.0.0, vantage-table 5.0.0, vantage-vista 3.0.0.

## 4.1.0 — 2026-11-20

- `date`, `time` and `timestamp` columns store and read the carrier forms of
  `vantage_types::temporal` (`Tag(1004)`, `Tag(101, "16:01:00.000000")`,
  `Tag(0, "2026-11-19T16:01:00.000000Z")`) from any accepted input. Before, only `date` was
  conformed (to `Tag(100)`), and times and instants were kept as given. A value that isn't its
  column's kind is refused on write and fails on read, naming the column.
- Conditions on those columns compare by value, whatever form the operand takes:
  `ts < "2026-11-19T17:01:00+01:00"` compares instants, not text.
- Watched rows are read the same way, so a Dio over a memory table sees the carrier forms.
- `seed::load_with`'s `prepare` callback returns `Result<()>`, so a seed file with a bad date
  fails to load.
- Tests: memory and a Dio over memory run `contract::check_temporal`.

## 4.0.2 — 2026-11-19

- Tests: the typed table runs `contract::check_list_condition`.

## 4.0.1 — 2026-11-17

- Fix: the id cell of a row keeps the id column's declared type. An id column declared `int`
  reads back `7`, not `"7"`, after a Vista write, and a typed table whose id field is `i64`
  stores an integer. The store's row key stays text, and an id cell that already names the key
  is kept as written.

## 4.0.0 — 2026-11-14

Requires vantage-table 4.0.0 and vantage-vista 2.11.0.

- **Typed tables.** A memory Vista is now built over `Table<MemoryDB>`, so table hooks and build
  injectors apply. `MemoryTableShell::new` and `MemoryCatalog` are gone; build through
  `MemoryVistaFactory`: `from_table` wraps a typed table, `table_from_spec` builds the typed
  table a spec describes, and `with_injector` and `with_resolver` attach a build injector and a
  spec resolver.
- `MemoryDB::in_transaction(&Transaction)`: a `MemoryDB` whose reads and writes go through the
  transaction, so a typed table joins it. A write still in flight when another task commits the
  transaction fails with "transaction finished" instead of landing on the replayed copy.
- **Rollups.** A `MemoryDB` table computes roll-up columns from its child rows on read, including
  an open transaction's writes: `MemoryRollup::new(column, target, foreign_key, op)` (with
  `with_source` and `with_conditions`; the struct is `#[non_exhaustive]`), added with
  `MemoryRollupExt::with_memory_rollup`. Reads, conditions and orders see the computed value,
  never one stored in the column, and writes return the computed value. Insert, patch and
  import check a row's membership with its roll-ups filled; an equality condition on a roll-up
  column is not written into inserted rows.
- A watch on a Vista whose table has roll-ups reports every write to the table, or to a table its
  roll-ups read, as `VistaChange::Invalidated`. Only tables the store holds when the watch starts
  are heard.
- Roll-up `sum`, `min`, `max` and `avg` (and `MemoryDB`'s sum, max and min) take decimal cells
  (`Tag(10)` text) alongside integers and floats. A decimal makes a sum or average a decimal,
  a float makes it a float, and so does a decimal sum past `Decimal`'s range; min and max
  compare across all three.
- A Vista built from a spec flags a column its injector made computed (a roll-up) `Calculated`.
- A reference to a missing table is an error, no longer not-found. With a resolver attached, a
  target it has no spec for is an error too.
- `import` writes nothing unless every record is written, hooks included.
- A memory Vista advertises `WritesReachHooks`.
- A Vista stores a declared `date` column date-only (`Tag(100)`, as fixtures load it) from any
  write: date text, or a datetime at midnight UTC such as `today()` or `add_months` returns, which
  is also what SurrealDB keeps a date as. A datetime with a time of day is stored as given. Dates
  order and compare as their text, and quicksearch and `Like` match it.
- `MemoryStore::fork`: an independent copy of the data (rows, indexes, id counters), with its own
  change channel and transaction claim, for running alternative futures from one state.
- A memory Vista takes `add_order`: later columns break the ties of the first, so a Rhai
  `.sort("a").sort("b")` sorts by both.

- Requires vantage-table 4.0.0. Roll-ups are planned on the `MemoryDB` doing the read and only
  for the columns a read selects, filters or sorts on: `MemoryRollupExt::with_lazy_memory_rollup`
  and `MemoryDB::check_rollups`. A roll-up column a read did not fill is left out of its rows. A
  roll-up over the table's own relation aggregates another column correctly; a column rolling
  up itself is an error.
- A condition over a related table (`MemoryCondition::DeferredIn`) reads that table through the
  `MemoryDB` resolving it, so inside a transaction it sees the transaction's rows. It holds a
  `RelatedRead` closure; `MemoryCondition::resolve`, `Query::resolve` and `table_query` take the
  `MemoryDB`.
- `Tag(101)` (time) compares as its text, like dates and datetimes.

## 3.1.1 — 2026-11-13

- Fix (VAN-432): an index keyed an integer above 2^53 apart from the float a scan calls equal to
  it, so an indexed lookup missed rows a scan found. Integers now key as their `f64` value.

## 3.1.0 — 2026-10-31

Requires vantage-vista 2.6.0.

- The store takes part in a `Transaction`. One transaction claims the store at a time; another one
  waits 2 s and then fails with a busy error. Writes go to a row journal that is replayed at
  commit, and dropped on rollback.

## 3.0.1 — 2026-10-26

- Tests of one behaviour are folded into one test each (`load_file` choosing YAML or JSON by
  extension, an index matching equal numbers) and tests another test already covers are dropped:
  128 tests become 125 with the same coverage. No API or behaviour change.

## 3.0.0 — 2026-10-18

Requires vantage-table 3.0.0.

### Breaking changes

- `MemoryCondition::DeferredIn { path, deferred }`: what `related_in_condition` now returns, so
  the condition names its column. It resolves like `Deferred`. Upgrade: an exhaustive `match` on
  `MemoryCondition` needs the new arm.

### Other changes

- `in_values_condition` builds an `InSet` comparison; `describe_condition` and `condition_column`
  are implemented, and the vista preview shows `col IN <deferred>`.

## 2.0.2 — 2026-10-15

- The tests use `vantage-diorama` 1.0.0, the proper version of the 0.15.0 rework. No change here.

## 2.0.1 — 2026-10-14

- The in-crate unit tests move to the blackbox suite under `tests/`, and tests that covered nothing
  another test didn't are dropped. No API or behaviour change.

## 2.0.0 — 2026-10-14

### Breaking changes

- The `vantage_memory::vista` module is private; `MemoryCatalog`, `MemoryTableShell`,
  `MemoryVistaFactory`, `MemoryVistaSpec`, `MemoryTableExtras` and `MemoryBlock` are at the crate
  root.
- `vista::Catalog` is `MemoryCatalog`; `new`, `store` and `register` are unchanged.
- `Catalog::get` is removed: `MemoryCatalog` implements `VistaResolver`, so `catalog.vista(name)`
  gives a Vista over the store table. A name neither registered nor in the store is not-found and
  creates no table.
- `MemoryTableShell::capabilities_for_memory` is crate-private; read `vista.capabilities()`.
- The shell implements `TableShell` 2.0: windows carry the total, and `fetch_page` and
  `set_page_size` are removed.
- A condition on a column the vista doesn't declare is an error.
- `MemoryCondition::Search(text)` is `Search { text, columns }`; `MemoryCondition::search(text)`
  builds one that reads every cell.
- A YAML column without `type:` is untyped (`original_type == None`).
- A join to a table that is neither built nor in the store is `is_not_found()` and creates no
  table; an undeclared relation is `unknown_relation`.
- A contained relation in hand-built metadata is `unsupported(Capability::Traverse)`.
- A serialised `Preview` has no `driver` field; `vista.driver_name()` is still `"memory"`.
- A write to a set narrowed by a dotted path (`root.leaf == v`) must hold `v` at `root.leaf`,
  otherwise it is `Conflict`.

### Other changes

- Seeded and written cells take their declared type's form (integer or text), and condition
  operands are conformed the same way, so an integer foreign key in a seed file joins a text id.
  A column without `type:` stores cells as given.
- On a column with no declared type, `=`/`!=` with an integer or its text matches either form.
- Multi-key `references:` (`keys:`) are accepted; traversal narrows by every key.
- Column-level `references:` are lowered through vantage-table's `spec_references`; they follow
  the top-level references in `metadata.references`, and a top-level entry of the same name wins.
- Quicksearch reads the columns flagged `Searchable`, or every column when none is, in the vista
  and the typed table alike.

Migration guide: https://vantage-ui.com/book/migration-2.0.html

## 1.2.2 — 2026-10-11

- Comparisons (conditions, order, min/max) read a datetime (`Tag(0, rfc3339)`) as its text.

## 1.2.1 — 2026-10-07

- Vista `patch` returns set-check errors other than "outside the set" instead of `NotFound`.
- Typed `insert_table_return_id_value` misfit error no longer carries an empty `id`.
- `MemoryDB::execute` builds on `vantage_expressions::execute_strict`; its errors name the backend in a `backend` context key.

## 1.2.0 — 2026-10-05

- **Breaking:** typed and Vista writes follow the write contract: insert of an existing id returns
  the stored row (an id outside the set is `Conflict`); delete of a missing row or one outside the
  set is `Ok`; replace of a missing row creates it; patch of a missing row is `NotFound`.
- Writes through a narrowed table or Vista are confined to its conditions and fill its equality
  conditions; has-many traversal fills the foreign key.
- **Breaking:** native import (`import_vista_values`) never overwrites: an existing id counts zero,
  and a row outside the set fails the import before anything is written.
- Depends on `vantage-table` 1.2.0, `vantage-dataset` 1.1.0.
- Vista shells advertise `can_confine_writes`.

## 1.1.1 — 2026-10-04

- The vista factory uses the shared spec helpers.
- Depends on `vantage-table` 1.1.1, `vantage-vista` 1.1.1.

## 1.1.0 — 2026-10-03

- Spec `lazy:` columns, computed by the Vista (`rhai` feature).
- Depends on `vantage-table` 1.1.0, `vantage-vista` 1.1.0.

## 1.0.0 — 2026-10-03

Initial release.

- In-memory datasource: sync store with per-table locks, `Arc`-shared rows and hash indexes.
- Typed `TableSource` (`MemoryDB`) with every `FilterOp`, search, ordering, paging, count/sum/min/max and references.
- Vista `MemoryTableShell` + `MemoryVistaFactory` (`memory: { indexed, seed }`), with native `watch_vista` and `upsert_vista_value`.
- Seed load/dump from JSON or YAML.
- `set_quiet` takes the table's write lock, so a write never loses its event or strands a `Reset` while quiet mode flips.
- A missing row is `ErrorKind::NotFound` on replace, patch and delete.
- Depends on `vantage-core` 1.0.0, `vantage-types` 1.0.0, `vantage-expressions` 1.0.0, `vantage-dataset` 1.0.0, `vantage-table` 1.0.0, `vantage-vista` 1.0.0.
