# vantage-faker 1.0 on vantage-memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move vantage-faker onto vantage-memory: generators seed `MemoryTable`s, Rhai sims write the store directly, and the legacy live mechanisms go away. This ships as faker 1.0.0.

**Architecture:**
- `DatasetGen` / `TableGen` seed a `MemoryStore` in reference order, quietly and reproducibly, using the existing `relational_rows`.
- `SimEngine` holds the store and resolves tables by name. Its data verbs call `MemoryTable` directly, adding `upsert` and `find`.
- Warm start uses the store's quiet mode and ends with `Reset`.
- `DatasetSpec` (behind the `serde` feature) is the shared YAML shape.
- The stress harness moves onto it and records `baseline-1.0.json`.

**Tech Stack:** Rust 2024; vantage-memory 0.6.1, vantage-rhai (feature `sim`), `fake`, ciborium, indexmap, serde / serde_yaml_ng.

**Spec:** `vantage-faker/SPEC.md` (read it first).

## Global Constraints

- Branch `faker/memory` in `/Users/rw/Work/vantage`, with no worktree. vantage-faker and vantage-faker-stress are standalone crates (each has an empty `[workspace]`). vantage-memory is a workspace member.
- vantage-faker stays at version **1.0.0**, unreleased. Its CHANGELOG `## 1.0.0 — 2026-09-29` block gains the new lines. vantage-memory goes to **0.6.1**.
- The storage is vantage-memory only: no `MockShell`, no `FakerCtx`, no faker broadcast channel.
- A failed sim ends, is counted as `errored`, and its rows stay. It is never restarted.
- Features: default is generators + seeding; `sim` is the Rhai engine (renamed from `rhai`); `serde` holds the config types.
- The default operations budget between sleeps is **5_000_000**, overridable per def with `ops`. `MAX_LIVE` stays 1000.
- `writes` counts:
  - `insert` always;
  - `upsert` when it returns `Inserted` or `Updated`;
  - `patch` / `set` / `delete` when the row existed.
- Files stay around 200 LOC or less. No `mod.rs` files. Comments are written for someone reading the code cold. Commits are one line, with no attribution trailer.
- **Execution rule from the user:** implementers write code and run only their task's tests. The controller runs `cargo fmt`, the full suites and clippy, and commits after each task. Long recorded runs (the baseline) are run by the controller.

## Spec deltas (made while planning; Task 7 folds them into SPEC.md)

1. **`insert` stores the script's map as given, plus the id.** There is no longer a declared-column shaping step, because a `MemoryTable` has no column list. Missing fields read as null anyway.
2. **The engine keeps the store alive.** The old "tables dropped → stop" check goes away, and dropping the engine stops the sims.
3. **Where `serde` applies.** `ColumnGen` keeps deserializing unconditionally, as it does today, and `serde` stays a normal dependency. The `serde` feature gates only the `config` module (`DatasetSpec` and its parts).
4. **Seeded row ids come from `seed_id(seq)`**, a zero-padded 20-digit string, as today. `relational_rows` assumes parent ids are `seed_id(0..parent_count)`. `DatasetGen` passes each referenced table's generated count as `parent_count`.

## Review Focus

- **A generator column named the same as the id column must not overwrite the seeded id.** Pinned in Task 2 (`id_column_generator_is_ignored`).
- **Two sims calling `upsert` with the same stable id** must leave one row, with the last write winning, and no duplicates. Pinned in Task 3 (`upsert_is_idempotent_across_spawns`).
- **`find` with a map naming a column no row has** returns an empty array, not an error. `find` with an empty map returns every id. Pinned in Task 3 (`find_edge_cases`).
- **A warm start whose sims wrote nothing** sends no `Reset`. Pinned in Task 3 (`warm_without_writes_sends_no_reset`).
- **A `DatasetSpec` table that references an unknown table** gives an error naming both tables, not a panic. Pinned in Task 5 (`unknown_reference_is_an_error`).

---

### Task 1: vantage-memory 0.6.1, the `set_quiet` race fix

**Files:**
- Modify: `vantage-memory/src/store/table.rs` (`set_quiet`)
- Modify: `vantage-memory/src/store/write_tests.rs` (add a test)
- Modify: `vantage-memory/Cargo.toml` (`version = "0.6.1"`)
- Modify: `vantage-memory/CHANGELOG.md` (new block `## 0.6.1 — 2026-09-29`)

**Interfaces:** `MemoryTable::set_quiet(&self, bool)`. The signature is unchanged; it now takes the rows write lock.

- [ ] **Step 1: Write the failing test** (`write_tests.rs`)

```rust
#[test]
fn quiet_toggles_never_lose_a_write() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let t = MemoryStore::new().table("t");
    let mut rx = t.subscribe();
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (t, stop) = (t.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut n = 0u64;
            while !stop.load(Ordering::Relaxed) && n < 3000 {
                t.upsert(&n.to_string(), Record::new());
                n += 1;
            }
            n
        })
    };
    for i in 0..400 {
        t.set_quiet(i % 2 == 0);
    }
    t.set_quiet(false);
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    while rx.try_recv().is_ok() {}
    // The race strands `missed = true` after quiet mode has ended, so an
    // empty quiet cycle would then send a spurious Reset.
    t.set_quiet(true);
    t.set_quiet(false);
    assert!(rx.try_recv().is_err(), "an empty quiet cycle sent an event");
    assert!(!t.is_quiet());
}
```

The race is timing-dependent, so run the test ten times: `for i in {1..10}; do cargo test -p vantage-memory --lib quiet_toggles; done`.

- [ ] **Step 2: Run it before the fix**

Run: `cargo test -p vantage-memory --lib quiet_toggles`
Expected: it fails at least intermittently. If it passes consistently, say so in the report, and still apply the fix, because the race is real by inspection.

- [ ] **Step 3: Fix.** In `set_quiet`, take `let _guard = self.rows.write();` before touching `quiet` / `missed`, and hold it across the swap and the `Reset` send. `changed()` always runs while a writer holds that lock, so the two can't interleave.

- [ ] **Step 4: Run the test.** `cargo test -p vantage-memory --lib quiet_toggles`, ten times. It passes every time.

- [ ] **Step 5: Version and CHANGELOG.** `vantage-memory/Cargo.toml` → `0.6.1`. Add this block at the top of the CHANGELOG:

```
## 0.6.1 — 2026-09-29

- `set_quiet` takes the table's write lock, so a write never loses its event or strands a `Reset` while quiet mode flips.
```

---

### Task 2: Seeding (`TableGen`, `DatasetGen`)

**Files:**
- Create: `vantage-faker/src/column.rs` (move `FakerColumn` out of `lib.rs` unchanged)
- Create: `vantage-faker/src/dataset.rs` (`DatasetGen`)
- Create: `vantage-faker/src/dataset/table.rs` (`TableGen`)
- Create: `vantage-faker/src/dataset/order.rs` (reference ordering and cycle detection)
- Create: `vantage-faker/src/dataset/tests.rs`
- Modify: `vantage-faker/Cargo.toml` (add `vantage-memory = { version = "0.6.1", path = "../vantage-memory" }`)
- Modify: `vantage-faker/src/lib.rs` (`mod column; pub mod dataset;` plus re-exports; keep everything else for now)

**Interfaces:**
- Consumes: `vantage_memory::{MemoryStore, MemoryTableHandle, TableDef}`, `MemoryTable::{upsert, set_quiet, id_column}`, and `relational_rows(values, columns, id_column, count, refs, fan_out)`, `Reference { column, parent_count }`, `FanOut`, `check_plan`, `ValueGen::from_seed`, `ColumnGen::validate`.
- Produces:
  - `TableGen::new(name)`, with builders `.id_column(s)`, `.column(FakerColumn)`, `.columns(impl IntoIterator<Item=FakerColumn>)`, `.count(n)`, `.reference(column, target_table)`, `.fan_out(FanOut)`, `.indexed(impl IntoIterator<Item=impl Into<String>>)`, and `name() -> &str`
  - `DatasetGen::new(seed: Option<u64>)`, `.table(TableGen)`, and `.generate(&self, store: &MemoryStore) -> Result<Vec<MemoryTableHandle>, String>` (tables in generation order)

`generate` rules (from the spec):
1. Validate each table: every `ColumnGen::validate()`, then `check_plan(refs, fan_out)`, where each ref's `parent_count` is the referenced table's `count`. Errors are prefixed `table <name>: ` and name the column.
2. Order the tables so that referenced ones come first, keeping declaration order otherwise. A cycle is an error: `reference cycle: a -> b -> a`. A reference to an undeclared table is an error: `table <t>: column <c> references unknown table <x>`.
3. For each table: `store.define(name, TableDef { id_column, indexed, id_prefix: None })`. If the returned table's `id_column()` differs, that's an error.
4. Set the table quiet, `upsert` every row from `relational_rows(&ValueGen::from_seed(seed_for(table)).with_rows(count), …)`, then unquiet. Seeded writes then send no events, and because each `upsert` inserts a new row, unquieting sends one `Reset`. That is harmless, since no one is watching yet, and it is documented.
5. A column whose name equals the id column is skipped when generating (the id comes from `seed_id`).

`seed_for(table)`: with `Some(seed)`, use `seed ^ hash(table name)` (FNV-1a over the bytes), so tables differ but stay reproducible. With `None`, use `None`.

- [ ] **Step 1: Write the failing tests** (`dataset/tests.rs`)

```rust
use vantage_memory::MemoryStore;

use crate::{ColumnGen, FakerColumn, FanOut};
use super::{DatasetGen, table::TableGen};

fn pick(values: &[&str]) -> ColumnGen {
    serde_yaml_ng::from_str(&format!("pick: {{ values: [{}] }}", values.join(", "))).unwrap()
}

fn gen() -> DatasetGen {
    DatasetGen::new(Some(42))
        .table(
            TableGen::new("invoice")
                .column(FakerColumn::new("client_id", "string"))
                .column(FakerColumn::new("status", "string").with_generator(pick(&["Open", "Paid"])))
                .reference("client_id", "client")
                .fan_out(FanOut { column: "client_id".into(), min: 1, max: 3 })
                .indexed(["client_id"]),
        )
        .table(TableGen::new("client").column(FakerColumn::new("name", "string")).count(5))
}

#[test]
fn referenced_tables_are_generated_first() {
    let store = MemoryStore::new();
    let tables = gen().generate(&store).unwrap();
    let names: Vec<&str> = tables.iter().map(|t| t.name()).collect();
    assert_eq!(names, ["client", "invoice"]);
    assert_eq!(store.table("client").len(), 5);
    let invoices = store.table("invoice");
    assert!((5..=15).contains(&invoices.len()));
    assert!(invoices.is_indexed("client_id"));
    let client_ids: Vec<String> = store.table("client").ids();
    for id in invoices.ids() {
        let row = invoices.get(&id).unwrap();
        let Some(ciborium::Value::Text(c)) = row.get("client_id") else { panic!("{row:?}") };
        assert!(client_ids.contains(c), "{c} is not a client id");
    }
}

#[test]
fn same_seed_same_rows() {
    let (a, b) = (MemoryStore::new(), MemoryStore::new());
    gen().generate(&a).unwrap();
    gen().generate(&b).unwrap();
    for t in ["client", "invoice"] {
        let ra: Vec<_> = a.table(t).ids().into_iter().map(|id| a.table(t).get(&id)).collect();
        let rb: Vec<_> = b.table(t).ids().into_iter().map(|id| b.table(t).get(&id)).collect();
        assert_eq!(ra, rb, "{t}");
    }
}

#[test]
fn seeding_is_quiet_then_resets() {
    let store = MemoryStore::new();
    let client = store.table("client");
    let mut rx = client.subscribe();
    DatasetGen::new(Some(1)).table(TableGen::new("client").count(3)).generate(&store).unwrap();
    let mut events = Vec::new();
    while let Ok(c) = rx.try_recv() {
        events.push(c);
    }
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(matches!(events[0], vantage_memory::MemoryChange::Reset));
}

#[test]
fn cycle_is_an_error() {
    let err = DatasetGen::new(None)
        .table(TableGen::new("a").column(FakerColumn::new("b_id", "string")).reference("b_id", "b"))
        .table(TableGen::new("b").column(FakerColumn::new("a_id", "string")).reference("a_id", "a"))
        .generate(&MemoryStore::new())
        .unwrap_err();
    assert!(err.contains("cycle"), "{err}");
}

#[test]
fn unknown_reference_is_an_error() {
    let err = DatasetGen::new(None)
        .table(TableGen::new("a").column(FakerColumn::new("x_id", "string")).reference("x_id", "x"))
        .generate(&MemoryStore::new())
        .unwrap_err();
    assert!(err.contains("a") && err.contains("x"), "{err}");
}

#[test]
fn count_zero_creates_an_empty_table() {
    let store = MemoryStore::new();
    DatasetGen::new(None).table(TableGen::new("empty")).generate(&store).unwrap();
    assert!(store.table_names().contains(&"empty".to_string()));
    assert_eq!(store.table("empty").len(), 0);
}

#[test]
fn existing_table_with_other_id_column_is_an_error() {
    let store = MemoryStore::new();
    store.define("t", vantage_memory::TableDef { id_column: "code".into(), ..Default::default() });
    let err = DatasetGen::new(None).table(TableGen::new("t").count(1)).generate(&store).unwrap_err();
    assert!(err.contains("code"), "{err}");
}

#[test]
fn invalid_generator_names_table_and_column() {
    let bad: ColumnGen = serde_yaml_ng::from_str("range: { min: 5, max: 1 }").unwrap();
    let err = DatasetGen::new(None)
        .table(TableGen::new("t").column(FakerColumn::new("n", "int").with_generator(bad)).count(1))
        .generate(&MemoryStore::new())
        .unwrap_err();
    assert!(err.contains("t") && err.contains("n"), "{err}");
}

#[test]
fn id_column_generator_is_ignored() {
    let store = MemoryStore::new();
    DatasetGen::new(Some(3))
        .table(TableGen::new("t").column(FakerColumn::new("id", "string").with_generator(pick(&["x"]))).count(2))
        .generate(&store)
        .unwrap();
    let ids = store.table("t").ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().all(|id| id != "x"));
}
```

If `ColumnGen::validate` doesn't reject `min > max` for `range`, pick a generator it does reject (see `generator/validate.rs`).

- [ ] **Step 2: Run the tests and check they fail.** `cargo test --features rhai --lib dataset` → compile errors.

- [ ] **Step 3: Implement** `column.rs`, `dataset/order.rs` (a Kahn topological sort that keeps declaration order among ready tables, with the cycle path in the error), `dataset/table.rs` and `dataset.rs`, following the rules above. Check the exact signature of `relational_rows` and whether it applies `with_rows` itself in `src/relational.rs`.

- [ ] **Step 4: Run the tests and check they pass.** `cargo test --features rhai --lib dataset`.

---

### Task 3: The sim engine over the store

**Files:**
- Modify: `vantage-faker/src/sim/builder.rs` (`.store(&MemoryStore)` replaces `.table(..)`; checks that every def's table exists; quiet warm start through the store)
- Modify: `vantage-faker/src/sim/kind.rs` (`Inner.store: MemoryStore` replaces `tables`; `set_quiet` iterates `store.table_names()`; remove `tables_alive`)
- Modify: `vantage-faker/src/sim/spawn.rs` and `current.rs` (drop the `tables_alive` checks; per-kind operations budget)
- Modify: `vantage-faker/src/sim.rs` (`SimDef.ops: Option<u64>` + `with_ops`; `pub const DEFAULT_OPS: u64 = 5_000_000`)
- Create: `vantage-faker/src/sim/vocab/convert.rs` (move `dynamic_to_cbor`, `cbor_to_dynamic`, `record_to_map`, `map_to_record` here from `rhai_effect.rs`, unchanged; point `random.rs` and `data.rs` at it)
- Modify: `vantage-faker/src/sim/vocab/data.rs` (verbs over `MemoryTable`; new `upsert`, `find`)
- Modify: `vantage-faker/src/sim/validate.rs` (`ops` must be greater than 0)
- Modify: `vantage-faker/src/sim/tests.rs` and `sim/tests/*.rs` (port the helpers to the store; new tests)
- Modify: `vantage-faker/examples/sims/*.rhai`, only where a verb changed (none should need to)

**Interfaces:**
- Consumes: `vantage_memory::{MemoryStore, MemoryTable, UpsertOutcome, Query, MemoryCondition}`, `FilterOp`.
- Produces:
  - `SimEngineBuilder::store(self, &MemoryStore) -> Self`. `.table(..)` is removed.
  - `SimDef::with_ops(self, u64) -> Self` and the field `pub ops: Option<u64>`.
  - `sim::DEFAULT_OPS`.
  - `SimEngine::stats()` is unchanged.

Behaviour:
- **`start()`:** every def's `table` must exist in the store (`store.table_names()` contains it), else `Err("sim <name>: table <t> does not exist")`. Then it validates, compiles and warm-starts as today, with warm quiet going through `Inner::set_quiet`. That loops `store.table_names()` and calls `MemoryTable::set_quiet`.
- **Verb table lookup:** only an existing name resolves (check `table_names()`, or keep a `HashSet` snapshot refreshed on miss). A missing table is a script error `no table <t> in this store`, which ends that sim.
- **`insert(t?, map)`:**
  - The id comes from the map's id-column value when present and non-empty. If so, call `MemoryTable::insert_as(id, record)`; a duplicate is a script error.
  - Otherwise call `insert(record)`.
  - Counts one write.
  - The record is `map_to_record(map)` with no shaping (spec delta 1).
- **`upsert(t?, id, map)` (new):** `MemoryTable::upsert(id, record)`. Counts a write when the outcome isn't `Unchanged`. Returns `()`.
- **`patch` / `set` / `delete`:** `patch(id, &partial)` / `delete(id)`. Count a write when the call returns `true`. A missing id is silent.
- **`get`, `ids`, `count`:** `get(id)` → map or `()`, `ids()`, `len()`.
- **`find(t?, map) -> Array` (new):** build a `Query` with one `MemoryCondition::cmp(k, FilterOp::Eq, dynamic_to_cbor(v))` per entry, run `query(&q)`, and return the ids. An empty map returns every id.
- **Operations budget:** `current::progress` compares against the running kind's `def.ops.unwrap_or(DEFAULT_OPS)` instead of `BACKGROUND_MAX_OPERATIONS`.

- [ ] **Step 1: Port the test helpers** (`sim/tests.rs`). They build a `MemoryStore` with the tables a test names:

```rust
fn store_with(tables: &[&str]) -> MemoryStore {
    let store = MemoryStore::new();
    for t in tables {
        store.table(t);
    }
    store
}

fn rows(table: &MemoryTable) -> Vec<Record<CborValue>> {
    table.ids().iter().filter_map(|id| table.get(id)).map(|r| (*r).clone()).collect()
}

/// An engine on the manual clock over one `log` table.
fn engine_with(defs: Vec<SimDef>) -> (SimEngine, MemoryTableHandle) {
    let store = store_with(&["log"]);
    let mut b = SimEngine::builder().store(&store).manual_clock(start()).seed(1);
    for d in defs {
        b = b.sim(d);
    }
    (b.start().expect("engine starts"), store.table("log"))
}
```

Keep `run_for`, `text` and `num` as they are. Update every test file under `sim/tests/` to these helpers. Assertions that read broadcast `ChangeEvent`s from a FakerCtx channel now subscribe to the `MemoryTable` (`table.subscribe()`) and match `MemoryChange` variants. Tests that relied on "tables dropped → engine stops" are deleted (spec delta 2); name each deleted test in your report.

- [ ] **Step 2: Add the new tests** (`sim/tests/store.rs`, registered in `sim/tests.rs`):

```rust
use super::*;

#[test]
fn upsert_is_idempotent_across_spawns() {
    let script = r#"upsert("fixed", #{ who: sim_id() }); upsert("fixed", #{ who: "same" });"#;
    let (engine, log) = engine_with(vec![SimDef::new("a", "log", script).with_spawn(3, 0.0, 3)]);
    run_for(&engine, 2, 1);
    assert_eq!(log.ids(), vec!["fixed"]);
    assert_eq!(text(&log.get("fixed").unwrap(), "who"), "same");
}

#[test]
fn find_matches_equalities_in_insertion_order() {
    let script = r#"
        insert(#{ id: "a", k: "x" }); insert(#{ id: "b", k: "y" }); insert(#{ id: "c", k: "x" });
        let hits = find(#{ k: "x" });
        insert(#{ id: "result", step: hits[0] + "," + hits[1] });
    "#;
    let (engine, log) = engine_with(vec![SimDef::new("a", "log", script)]);
    run_for(&engine, 1, 1);
    assert_eq!(text(&log.get("result").unwrap(), "step"), "a,c");
}

#[test]
fn find_edge_cases() {
    let script = r#"
        insert(#{ id: "a", k: "x" });
        insert(#{ id: "none", step: "" + find(#{ nope: 1 }).len() });
        insert(#{ id: "all", step: "" + find(#{}).len() });
    "#;
    let (engine, log) = engine_with(vec![SimDef::new("a", "log", script)]);
    run_for(&engine, 1, 1);
    assert_eq!(text(&log.get("none").unwrap(), "step"), "0");
    assert_eq!(text(&log.get("all").unwrap(), "step"), "2");
}

#[test]
fn sims_write_a_relational_table() {
    let store = MemoryStore::new();
    crate::dataset::DatasetGen::new(Some(5))
        .table(crate::dataset::TableGen::new("client").count(3))
        .table(
            crate::dataset::TableGen::new("invoice")
                .column(crate::FakerColumn::new("client_id", "string"))
                .reference("client_id", "client")
                .count(4),
        )
        .generate(&store)
        .unwrap();
    let script = r#"for id in ids() { patch(id, #{ paid: true }); }"#;
    let engine = SimEngine::builder()
        .store(&store)
        .sim(SimDef::new("payer", "invoice", script))
        .manual_clock(start())
        .start()
        .unwrap();
    run_for(&engine, 1, 1);
    let inv = store.table("invoice");
    assert!(inv.ids().iter().all(|id| inv.get(id).unwrap().get("paid") == Some(&CborValue::Bool(true))));
}

#[test]
fn missing_default_table_fails_start() {
    let err = SimEngine::builder()
        .store(&store_with(&["log"]))
        .sim(SimDef::new("a", "nope", "sleep(seconds(1));"))
        .manual_clock(start())
        .start()
        .err()
        .expect("start fails");
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn warm_start_ends_with_one_reset_per_written_table() {
    let store = store_with(&["log", "other"]);
    let mut log_rx = store.table("log").subscribe();
    let mut other_rx = store.table("other").subscribe();
    let def = SimDef::new("w", "log", "insert(#{ who: \"warm\" }); sleep(minutes(1));")
        .with_warm(Duration::from_secs(600))
        .with_spawn(2, 0.0, 2);
    let _engine = SimEngine::builder().store(&store).sim(def).manual_clock(start()).start().unwrap();
    let log_events: Vec<_> = std::iter::from_fn(|| log_rx.try_recv().ok()).collect();
    assert!(matches!(log_events.as_slice(), [vantage_memory::MemoryChange::Reset]), "{log_events:?}");
    assert!(other_rx.try_recv().is_err(), "an untouched table gets no Reset");
}

#[test]
fn warm_without_writes_sends_no_reset() {
    let store = store_with(&["log"]);
    let mut rx = store.table("log").subscribe();
    let def = SimDef::new("idle", "log", "sleep(minutes(5));").with_warm(Duration::from_secs(600));
    let _engine = SimEngine::builder().store(&store).sim(def).manual_clock(start()).start().unwrap();
    assert!(rx.try_recv().is_err());
}

#[test]
fn ops_budget_ends_a_spinning_sim() {
    let def = SimDef::new("spin", "log", "let n = 0; loop { n += 1; }").with_ops(100_000);
    let (engine, _log) = engine_with(vec![def]);
    run_for(&engine, 1, 1);
    let s = engine.stats();
    assert_eq!((s.live, s.errored), (0, 1));
}

#[test]
fn writes_count_only_changing_sim_writes() {
    let script = r#"
        insert(#{ id: "a" });           // 1
        upsert("a", #{ id: "a" });      // unchanged → 0
        upsert("b", #{ x: 1 });         // 1
        patch("a", #{ x: 2 });          // 1
        patch("missing", #{ x: 2 });    // 0
        delete("b");                    // 1
        delete("b");                    // 0
    "#;
    let (engine, _log) = engine_with(vec![SimDef::new("a", "log", script)]);
    run_for(&engine, 1, 1);
    assert_eq!(engine.stats().writes, 4);
}
```

`upsert("a", #{ id: "a" })` after `insert(#{ id: "a" })` is `Unchanged` only if the stored row is exactly `{id: "a"}`. That holds with no shaping (spec delta 1).

- [ ] **Step 3: Run the tests and check they fail.** `cargo test --features rhai --lib sim` → compile errors.

- [ ] **Step 4: Implement**, following the behaviour list. Keep `rhai_effect.rs` compiling for now: it keeps its own copies of the helpers until Task 4 deletes it, or re-export them from `sim/vocab/convert.rs` if it only needs them under the `rhai` feature.

- [ ] **Step 5: Run the tests and check they pass.** `cargo test --features rhai --lib sim`.

---

### Task 4: Remove the legacy modules and rename the feature

**Files:**
- Delete:
  - `vantage-faker/src/effect.rs`, `src/handle.rs`, `src/flights.rs` + `src/flights/`, `src/pulse.rs`, `src/live_folder.rs` + `src/live_folder/`, `src/rhai_effect.rs`
  - `examples/fifo_cli.rs`, `examples/scenery_cli.rs`, `examples/scenery_folder_cli.rs`, `examples/live_folder_cli.rs`
  - `tests/bdd.rs` + `tests/features/` (the live_folder BDD) and the `[[test]] bdd` entry
  - `tests/shaped_backends.rs`, only if it is built on `FakerTable`. Otherwise port it to build its Vista over a `MemoryTableShell`; see the note below.
- Modify: `vantage-faker/src/lib.rs`. Remove `FakerTable`, `faker_metadata` and `EVENT_CAPACITY`, and the module declarations and re-exports of everything deleted. New re-exports:
  - `pub use column::FakerColumn;`
  - `pub use dataset::{DatasetGen, TableGen};`
  - `#[cfg(feature = "sim")] pub use sim::{DEFAULT_OPS, SimDef, SimEngine, SimEngineBuilder, SimStats, Spawn};`
  - the generator, relational, shape and value_gen re-exports as today
  
  Update the crate doc comment to describe generators → seeding into vantage-memory → sims.
- Modify: `vantage-faker/src/shape.rs`, doc comments only (they mention `FakerCtx` / `MockShell`). `ShapedShell` keeps wrapping any `Box<dyn TableShell>`.
- Modify: `vantage-faker/Cargo.toml`:
  - rename feature `rhai = ["dep:vantage-rhai"]` to `sim = ["dep:vantage-rhai"]`
  - drop dependencies that only the deleted code used (`async-trait` if unused; `tokio` features `time`/`macros` if unused; `vantage-diorama`, which is used only for `ChangeEvent` in deleted code, so check)
  - drop dev-dependencies no longer used: `cucumber`, `vantage-cli-util`, `vantage-vista-factory`
  - update `description` to "Synthetic data for Vantage: column generators seeding vantage-memory tables, and Rhai sims that mutate them live"
- Modify: `vantage-faker/src/sim.rs` and `sim/*`: `#[cfg(feature = "rhai")]` → `sim` wherever it appears in the crate.
- Modify: `vantage-faker/README.md`. Rewrite it for the new shape:
  1. generators and `DatasetGen` (with an example)
  2. sims (the builder, the verbs table including `upsert`/`find`, warm start and `Reset`, the `ops` budget, and failure = end)
  3. `ShapedShell`
  4. features

  Drop the effects, pulse, live_folder and flights sections. Plain engineer prose.

`tests/shaped_backends.rs`: if it needs a store-backed shell, build one with `vantage_memory::MemoryTableShell::new(store.table(name), metadata, Catalog::new(store.clone()))` (add `vantage-memory` as a dev-dependency path if needed) and wrap it in `ShapedShell`. Keep its assertions.

- [ ] **Step 1: Delete and rewire** as listed.
- [ ] **Step 2: Build.** Run `cargo build`, `cargo build --features sim` and `cargo build --examples --features sim`, one at a time. All three are clean.
- [ ] **Step 3: Run the crate's tests.** `cargo test --features sim` passes. Report the per-target counts.

---

### Task 5: Config types (`serde` feature)

**Files:**
- Create: `vantage-faker/src/config.rs` (`DatasetSpec`, `TableSpec`, `ColumnSpec`, `FanOutSpec`, `generate`)
- Create: `vantage-faker/src/config/sims.rs` (`SimSpec`, `SpawnSpec`, `sim_defs`, `start_sims`, `parse_duration`)
- Create: `vantage-faker/src/config/tests.rs`
- Modify: `vantage-faker/Cargo.toml` (`serde = []` feature; move `serde_yaml_ng` from dev-dependencies to a normal dependency only if `config` needs it outside tests; it shouldn't)
- Modify: `vantage-faker/src/lib.rs` (`#[cfg(feature = "serde")] pub mod config;`)

**Interfaces:**
- Consumes: `DatasetGen`, `TableGen`, `FakerColumn`, `ColumnGen`, `FanOut`, `SimDef`, `SimEngine`.
- Produces:
  - `DatasetSpec { seed: Option<u64>, tables: IndexMap<String, TableSpec>, sims: IndexMap<String, SimSpec> }`
  - `TableSpec { id_column: Option<String>, count: usize, columns: IndexMap<String, ColumnSpec>, indexed: Vec<String>, references: IndexMap<String, String>, fan_out: Option<FanOutSpec> }`
  - `ColumnSpec { ty: Option<String> (serde rename "type"), faker: Option<ColumnGen> }`
  - `FanOutSpec { column: String, min: usize, max: usize }`
  - `SimSpec { table: Option<String>, script: String, clock: Option<f64>, warm: Option<String>, ops: Option<u64>, spawn: SpawnSpec }`
  - `SpawnSpec { burst: Option<usize>, rate: Option<f64>, max: Option<usize>, args: IndexMap<String, serde_json::Value> }`
  - every struct derives `Debug, Clone, Deserialize`, with `#[serde(deny_unknown_fields)]` and `#[serde(default)]` where the field is optional
  - `DatasetSpec::generate(&self, store: &MemoryStore) -> Result<(), String>`
  - `#[cfg(feature = "sim")] DatasetSpec::sim_defs(&self) -> Result<Vec<SimDef>, String>`
  - `#[cfg(feature = "sim")] DatasetSpec::start_sims(&self, store: &MemoryStore) -> Result<Option<SimEngine>, String>`, which returns `None` when `sims` is empty and otherwise seeds the builder with `self.seed`
  - `pub fn parse_duration(&str) -> Result<Duration, String>`: `500ms`, `1.5s`, `2m`, `6h`, `3d` or bare seconds

Defaults, as in vantage-ui: `burst: 1`, `rate: 0`, `max: 1`, `clock: 1`, and a sim with no `table` uses the first table in `tables`. `sim_defs` with no tables and a sim missing `table:` is an error.

- [ ] **Step 1: Write the failing tests** (`config/tests.rs`)

```rust
use vantage_memory::MemoryStore;

use super::*;

const YAML: &str = r#"
seed: 9
tables:
  client:
    count: 3
    columns:
      name: { type: string, faker: { pick: { values: [Ann, Bob] } } }
  invoice:
    count: 0
    indexed: [client_id]
    references: { client_id: client }
    fan_out: { column: client_id, min: 1, max: 2 }
    columns:
      client_id: {}
sims:
  pay:
    table: invoice
    script: "sleep(seconds(1));"
    clock: 10
    warm: 2h
    ops: 1000000
    spawn: { burst: 2, rate: 1.5, max: 4, args: { who: bob } }
  idle: { script: "sleep(seconds(1));" }
"#;

fn spec() -> DatasetSpec {
    serde_yaml_ng::from_str(YAML).unwrap()
}

#[test]
fn parses_and_generates() {
    let store = MemoryStore::new();
    spec().generate(&store).unwrap();
    assert_eq!(store.table("client").len(), 3);
    assert!((3..=6).contains(&store.table("invoice").len()));
    assert!(store.table("invoice").is_indexed("client_id"));
}

#[test]
fn unknown_keys_are_rejected() {
    let err = serde_yaml_ng::from_str::<DatasetSpec>("tables: { t: { cnt: 1 } }").unwrap_err().to_string();
    assert!(err.contains("cnt"), "{err}");
}

#[test]
fn unknown_reference_is_an_error() {
    let s: DatasetSpec = serde_yaml_ng::from_str("tables: { a: { count: 1, references: { x_id: x }, columns: { x_id: {} } } }").unwrap();
    let err = s.generate(&MemoryStore::new()).unwrap_err();
    assert!(err.contains("a") && err.contains("x"), "{err}");
}

#[test]
fn durations_parse() {
    use std::time::Duration;
    assert_eq!(parse_duration("500ms").unwrap(), Duration::from_millis(500));
    assert_eq!(parse_duration("2h").unwrap(), Duration::from_secs(7200));
    assert!(parse_duration("soon").is_err());
}

#[cfg(feature = "sim")]
#[test]
fn sim_defs_apply_defaults() {
    let defs = spec().sim_defs().unwrap();
    let pay = &defs[0];
    assert_eq!((pay.spawn.burst, pay.spawn.rate_per_min, pay.spawn.max), (2, 1.5, 4));
    assert_eq!((pay.clock, pay.ops), (10.0, Some(1_000_000)));
    assert_eq!(pay.warm, Some(std::time::Duration::from_secs(7200)));
    assert_eq!(pay.spawn.args["who"], "bob");
    let idle = &defs[1];
    assert_eq!(idle.table, "client");
    assert_eq!((idle.spawn.burst, idle.spawn.rate_per_min, idle.spawn.max, idle.clock), (1, 0.0, 1, 1.0));
}

#[cfg(feature = "sim")]
#[test]
fn start_sims_runs_after_generate() {
    let store = MemoryStore::new();
    let s: DatasetSpec = serde_yaml_ng::from_str(
        "tables: { log: { count: 0 } }\nsims: { w: { script: \"insert(#{ who: 1 }); sleep(minutes(5));\" } }",
    )
    .unwrap();
    s.generate(&store).unwrap();
    let engine = s.start_sims(&store).unwrap().expect("an engine");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(store.table("log").len(), 1);
    drop(engine);
    let empty: DatasetSpec = serde_yaml_ng::from_str("tables: { t: {} }").unwrap();
    assert!(empty.start_sims(&store).unwrap().is_none());
}
```

- [ ] **Step 2: Run the tests and check they fail.** `cargo test --features "serde sim" --lib config` → compile errors.
- [ ] **Step 3: Implement** `config.rs` and `config/sims.rs`. `generate` maps each `TableSpec` to a `TableGen`. Columns are `FakerColumn::new(name, ty.unwrap_or("string"))` plus the generator; references and fan-out map across. It then calls `DatasetGen::new(self.seed).generate(store)`.
- [ ] **Step 4: Run the tests and check they pass.** Run `cargo test --features "serde sim" --lib config`, then `cargo test --features serde --lib config`. The second run shows that the non-sim tests compile without `sim`.

---

### Task 6: Move the stress harness onto faker 1.0

**Files (`vantage-faker-stress/`):**
- Modify: `Cargo.toml`. Change the faker features to `["sim", "serde"]`, and add `vantage-memory = { version = "0.6.1", path = "../vantage-memory" }`.
- Modify: `src/scenario.rs` + `src/scenario/build.rs`. Replace the private `TableSpec` / `ColumnSpec` / `SimSpec` / `SpawnSpec` with `vantage_faker::config::{DatasetSpec, TableSpec, SimSpec}`. `Scenario` becomes `{ dataset: DatasetSpec (serde flatten), stress: StressSpec }`. `seed`, `tables` and `sims` stay at the top level of the YAML. Check that `deny_unknown_fields` works with `flatten`; if it doesn't, keep explicit fields `seed`, `tables` and `sims` typed with faker's structs.
  - `scaled`, `without_warm` and `max_total` operate on those structs.
  - `sim_defs` delegates to `DatasetSpec::sim_defs`.
  - `faker_columns` goes away.
  - `parse_duration` re-exports faker's.
- Modify: `src/runner.rs`.
  - Build a `MemoryStore`, call `dataset.generate(&store)`, and attach the loads.
  - Start the engine with `SimEngine::builder().store(&store)`, the defs and the seed, in `spawn_blocking` as today.
  - Stats are unchanged.
- Modify: `src/load.rs`.
  - `attach` takes `(table: MemoryTableHandle, metadata: VistaMetadata, store: &MemoryStore, lens: Option<&Arc<Lens>>)`.
  - The counting subscriber uses `table.subscribe()` (`MemoryChange`, with `Reset` counted as delivered).
  - The drain probe's backlog comes from `Receiver::len()` instead of `Sender::len()`.
  - With a lens, the Dio is made from `Vista::new(name, Box::new(MemoryTableShell::new(table, metadata, Catalog::new(store.clone()))))` and goes live with `dio.watch().await`, with no `handle_event`.
  - The TableScenery stays as is.
  - Metadata comes from the scenario's column names, with the id column flagged.
- Modify: `tests/*.rs`, `src/**/tests.rs` and the `scenarios/**/scenario.yaml` files, only where the schema changed. They shouldn't need to.
- Modify: `README.md`. Update the "Writing a scenario" section to point at faker's `DatasetSpec` fields (`indexed`, `references`, `fan_out`, `ops` are now available).

- [ ] **Step 1: Port**, as listed.
- [ ] **Step 2: Run the harness's own tests.** `cargo test` in `vantage-faker-stress` (lib + runner + smoke; the chaos test stays ignored). They pass.
- [ ] **Step 3: One live run.** `cargo run -- run churn --duration 5s --dio` prints samples with non-zero `events/s`.

---

### Task 7: Baseline 1.0, CHANGELOG and spec sync (the controller runs the recordings)

**Files:**
- Create: `vantage-faker-stress/baseline-1.0.json`
- Modify: `vantage-faker-stress/README.md` (a "Baseline (1.0)" section, compared with 0.7)
- Modify: `vantage-faker/CHANGELOG.md` (extend the `## 1.0.0 — 2026-09-29` block)
- Modify: `vantage-faker/SPEC.md` (fold in the four spec deltas, plus whatever implementation changed)

- [ ] **Step 1 (controller): record.** Run each command separately, in a debug build, with nothing heavy running alongside:
  ```
  cargo run -- ramp idle --steps 100,250,500,1000 --hold 15s --json /tmp/c-idle.json
  cargo run -- ramp churn --steps 100,200,400,800 --hold 20s --dio --json /tmp/c-churn.json
  cargo run -- ramp swarm --steps 100,250,500,1000 --hold 20s --dio --json /tmp/c-swarm.json
  cargo run -- ramp sweeper --steps 100,250,500,1000 --hold 20s --dio --json /tmp/c-sweeper.json
  cargo run -- run warm --json /tmp/c-warm.json
  cargo run -- run chaos/flood --json /tmp/c-flood.json
  cargo test --test smoke -- --ignored --nocapture
  ```
  Then `jq -s '.' /tmp/c-{idle,churn,swarm,sweeper,warm,flood}.json > vantage-faker-stress/baseline-1.0.json`.

- [ ] **Step 2: README baseline section.** Write 5–10 lines comparing 1.0 with 0.7, using `compare` on the extracted reports (`jq '.[i]'`): idle cost per sim, the churn top step, swarm vs sweeper, warm-start time, the flood `lagged` count and lag, the chaos verdicts, and how long `spin` ran. Every number must be traceable to the two JSON files.

- [ ] **Step 3: CHANGELOG.** Append to the `1.0.0` block (keep the existing `SimEngine::stats` line):

```
- Storage is vantage-memory: `DatasetGen` / `TableGen` seed `MemoryTable`s in reference order, quiet and reproducible; relational tables are ordinary writable tables.
- Removed: `FakerTable`, `FakerCtx`, `FakerHandle`, `FakerEffect` (static/fifo), flights, `PulseSim`, `LiveFolderSim`, `RhaiEffect`.
- `SimEngine` runs over a `MemoryStore` (`builder().store(..)`); sims reach tables by name.
- Sim verbs `upsert(t?, id, map)` and `find(t?, #{col: val})`; `insert` stores the map as given.
- Warm start ends with one `Reset` per written table.
- Operations budget between sleeps defaults to 5M, per def `ops`.
- Feature `rhai` renamed to `sim`; feature `serde` adds `config::{DatasetSpec, TableSpec, SimSpec}`.
```

- [ ] **Step 4: SPEC sync.** Apply the four spec deltas, plus the facts listed in the Task 3–6 reports, to the matching SPEC sections.

- [ ] **Step 5 (controller): final checks.** Run each separately:
  - `cargo test --features "sim serde"` in vantage-faker
  - `cargo clippy --features "sim serde" --all-targets` in vantage-faker (0 warnings)
  - `cargo test -p vantage-memory`
  - `cargo test` in vantage-faker-stress
