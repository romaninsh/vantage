# Changelog

## 4.0.0 — 2026-11-01

Requires vantage-core 2.0.0, vantage-vista 3.0.0 and vantage-table 4.0.0.

- The vista advertises `DeleteAll` next to `Delete`.

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
