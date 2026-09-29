# vantage-memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A fast in-memory vantage datasource with a typed `TableSource` layer and a Vista `TableShell` layer over one store, including native change events (`watch_vista`).

**Architecture:**
- **Store:** a synchronous core `MemoryStore` holds named tables. Each table has its own lock, `Arc`-shared rows, optional hash indexes and a broadcast channel of changes.
- **Queries:** one `Query` plus the `MemoryCondition` evaluator serve both layers.
- **Typed layer:** `MemoryDB` implements `TableSource`, following vantage-redb's patterns.
- **Vista layer:** `MemoryTableShell` implements `TableShell` directly over the store, and `MemoryVistaFactory` builds Vistas from specs.

**Tech Stack:** Rust 2024, ciborium, indexmap, parking_lot, tokio (`sync`), async-trait, async-stream, serde; vantage-core/types/expressions/dataset/table/vista 0.6.

**Spec:** `vantage-memory/SPEC.md` (read it first).

**Reference for trait signatures:** `/Users/rw/Work/vantage/.superpowers/memory-ref/traits.md`. It is scratch and never committed. It holds verbatim signatures of `TableSource`, `TableShell`, `VistaFactory`, `VistaCapabilities` and `FilterOp`, plus vantage-redb's type system, condition and operation patterns, csv's factory, surreal's `watch_vista`, and diorama's live-Dio test pattern. Tasks point at its sections by number (§N).

## Global Constraints

- Work in `/Users/rw/Work/vantage` on branch `memory/datasource`. No worktree.
- The crate is `vantage-memory`, version `0.6.0`, edition 2024, and a **member** of the root workspace. Its internal deps use the `{ version = "0.6", path = "../<crate>" }` form. `license`/`authors`/`homepage`/`repository` are copied from vantage-redb's Cargo.toml.
- Files stay around 200 LOC or less, one responsibility each. There are no `mod.rs` files (`foo.rs` + `foo/`).
- Comments are written for someone reading the code cold. No change narration, no "for faker/demo".
- Commit messages are one line (`-m "..."`), with no attribution trailer.
- Run `cargo fmt -p vantage-memory` before each commit. `cargo clippy -p vantage-memory --all-targets` stays warning-free.
- Don't chain slow cargo commands with `&&`.
- Mocks (`MockShell`, `MockTableSource`, `ImDataSource`) are not used by vantage-memory's non-test code and are not modified.
- No other crate is modified, except the root `Cargo.toml` `members` list.
- Writes that change nothing send no event and don't count as writes.
- The event channel capacity is 4096.
- Generated ids are a per-table counter: `"1"`, `"2"`, … or `"<prefix>1"`.

## Spec deltas (made while planning; Task 9 folds them into SPEC.md)

1. `MemoryChange` has precise variants: `Inserted { id, row }`, `Updated { id, row, old }`, `Deleted { id, old }`. It no longer uses `Option` fields.
2. **One lock per table.** A table's rows and indexes share one `RwLock`, so index updates can't drift from the rows.
3. **Order keys.** The typed layer's `Table::orders()` yields `(Condition, SortDirection)`, so `MemoryCondition` gains a `Column(String)` variant. It is a bare column reference, valid only as an order key; evaluating it as a filter is an error. `MemoryOperation` adds `.ascending()` / `.descending()` returning `OrderBy<MemoryCondition>`.
4. **Sort direction.** `Query.order` uses `vantage_vista::SortDirection`, and the typed layer maps `vantage_table::sorting::SortDirection` onto it. Nulls sort first ascending and last descending, because descending reverses the whole ordering.
5. **The Vista shell sits directly on the store** (`MemoryTableHandle` + `Query`), not on a typed `Table`.
6. **Reference targets.** The factory keeps a shared catalog of built tables' `VistaMetadata`. `get_ref`/`get_ref_target` use it to give the target Vista its columns. An unregistered target gets id-only metadata.

## Review Focus

- **Two rows equal as numbers but not as CBOR** (`Integer(1)` vs `Float(1.0)`) must match `Eq`/`InSet` on both the scan and the index path. Pinned in Task 3 (`eq_int_matches_float`) and Task 4 (`index_matches_int_and_integral_float`).
- **A generated id colliding with a caller-supplied one.** The caller inserts `"3"`, and then three generated inserts happen. The counter must skip `"3"`, not error or overwrite. Pinned in Task 1 (`generated_ids_skip_supplied_ones`).
- **A patch that sets the same values.** It returns `true`, sends no event and counts no write. Pinned in Task 1 (`noop_patch_sends_nothing`).
- **A filtered Vista watching a row that moves out of its filter.** The watcher sees `Deleted`. A row moving in is seen as `Inserted`. Pinned in Task 8 (`row_leaving_filter_is_deleted`, `row_entering_filter_is_inserted`).
- **Paging past the end, or with `limit` 0.** The result is an empty Vec, never a panic. Pinned in Task 3 (`offset_past_end_is_empty`).

---

### Task 1: Crate scaffold and the store core

**Files:**
- Modify: `Cargo.toml` (root): add `"vantage-memory"` to `members`, after `"vantage-redb"`
- Create: `vantage-memory/Cargo.toml`
- Create: `vantage-memory/src/lib.rs`
- Create: `vantage-memory/src/store.rs`
- Create: `vantage-memory/src/store/table.rs`
- Create: `vantage-memory/src/store/ids.rs`
- Create: `vantage-memory/src/store/events.rs`
- Test: `vantage-memory/src/store/tests.rs` (registered as `#[cfg(test)] mod tests;` in `store.rs`)

**Interfaces:**
- Produces:
  - `pub type Row = Arc<Record<CborValue>>`, where `CborValue = ciborium::Value`
  - `MemoryChange` (in `store/events.rs`, `#[derive(Clone, Debug)]`): `Inserted { id: String, row: Row }`, `Updated { id: String, row: Row, old: Row }`, `Deleted { id: String, old: Row }`, and `MemoryChange::id(&self) -> &str`
  - `TableDef { id_column: String, indexed: Vec<String>, id_prefix: Option<String> }`, with `Default` giving `id_column = "id"`
  - `MemoryStore`: `Clone`, `Default`, `new()`, `table(&self, name) -> MemoryTableHandle` (creates with default def), `define(&self, name, TableDef) -> MemoryTableHandle` (replaces the def only if the table doesn't exist yet), `table_names(&self) -> Vec<String>`
  - `pub type MemoryTableHandle = Arc<MemoryTable>`
  - `MemoryTable` methods:
    - `name() -> &str`, `id_column() -> &str`
    - `insert(Record<CborValue>) -> vantage_core::Result<String>`
    - `upsert(&str, Record<CborValue>)`
    - `patch(&str, &Record<CborValue>) -> bool`, `delete(&str) -> bool`
    - `get(&str) -> Option<Row>`, `ids() -> Vec<String>`, `len() -> usize`, `is_empty() -> bool`
    - `set_quiet(bool)`, `is_quiet() -> bool`, `writes() -> u64`
    - `subscribe() -> tokio::sync::broadcast::Receiver<MemoryChange>`
  - private `struct Rows { map: IndexMap<String, Row> }` inside `table.rs`. Task 4 adds an `indexes` field.

- [ ] **Step 1: Scaffold**

Root `Cargo.toml`: add `"vantage-memory",` to `members`, after `"vantage-redb",`.

`vantage-memory/Cargo.toml`:

```toml
[package]
name = "vantage-memory"
version = "0.6.0"
edition = "2024"
license = "MIT OR Apache-2.0"
authors = ["Romans Malinovskis <me@nearly.guru>"]
description = "Fast in-memory datasource for Vantage: typed tables, Vista shells and live change events"
documentation = "https://docs.rs/vantage-memory"
homepage = "https://romaninsh.github.io/vantage"
repository = "https://github.com/romaninsh/vantage"
readme = "README.md"

[dependencies]
vantage-core = { version = "0.6", path = "../vantage-core" }
vantage-types = { version = "0.6", path = "../vantage-types" }
vantage-expressions = { version = "0.6", path = "../vantage-expressions" }
vantage-dataset = { version = "0.6", path = "../vantage-dataset" }
vantage-table = { version = "0.6", path = "../vantage-table" }
vantage-vista = { version = "0.6", path = "../vantage-vista" }
ciborium = { version = "0.2", features = ["std"] }
indexmap = { version = "2", features = ["serde"] }
parking_lot = "0.12"
tokio = { version = "1", features = ["sync"] }
async-trait = "0.1"
async-stream = "0.3"
futures-core = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml_ng = "0.10"
paste = "1"

[dev-dependencies]
tokio = { version = "1", features = ["full"] }
vantage-diorama = { version = "0.13", path = "../vantage-diorama" }
futures-util = "0.3"
tempfile = "3"
```

`vantage-memory/src/lib.rs`:

```rust
//! Fast in-memory datasource for Vantage. See SPEC.md.

pub mod store;

pub use store::{MemoryChange, MemoryStore, MemoryTable, MemoryTableHandle, Row, TableDef};
```

- [ ] **Step 2: Write the failing tests**

`vantage-memory/src/store/tests.rs`:

```rust
use ciborium::Value as CborValue;
use vantage_types::Record;

use super::*;

fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}
fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}

#[test]
fn insert_generates_counter_ids_and_sets_id_column() {
    let t = MemoryStore::new().table("ticket");
    let a = t.insert(rec(&[("status", text("Open"))])).unwrap();
    let b = t.insert(rec(&[("status", text("Closed"))])).unwrap();
    assert_eq!((a.as_str(), b.as_str()), ("1", "2"));
    assert_eq!(t.get("1").unwrap().get("id"), Some(&text("1")));
    assert_eq!(t.ids(), vec!["1", "2"]);
}

#[test]
fn insert_uses_supplied_id_and_rejects_duplicates() {
    let t = MemoryStore::new().table("ticket");
    assert_eq!(t.insert(rec(&[("id", text("T-9"))])).unwrap(), "T-9");
    assert!(t.insert(rec(&[("id", text("T-9"))])).is_err());
    let n = t.insert(rec(&[("id", CborValue::Integer(7.into()))])).unwrap();
    assert_eq!(n, "7");
}

#[test]
fn generated_ids_skip_supplied_ones() {
    let t = MemoryStore::new().table("ticket");
    t.insert(rec(&[("id", text("2"))])).unwrap();
    let ids: Vec<String> = (0..3).map(|_| t.insert(Record::new()).unwrap()).collect();
    assert_eq!(ids, vec!["1", "3", "4"]);
}

#[test]
fn id_prefix_applies_to_generated_ids() {
    let s = MemoryStore::new();
    let t = s.define("ticket", TableDef { id_prefix: Some("T-".into()), ..TableDef::default() });
    assert_eq!(t.insert(Record::new()).unwrap(), "T-1");
}

#[test]
fn upsert_inserts_then_replaces() {
    let t = MemoryStore::new().table("ticket");
    let mut rx = t.subscribe();
    t.upsert("a", rec(&[("n", CborValue::Integer(1.into()))]));
    t.upsert("a", rec(&[("n", CborValue::Integer(2.into()))]));
    assert!(matches!(rx.try_recv().unwrap(), MemoryChange::Inserted { .. }));
    assert!(matches!(rx.try_recv().unwrap(), MemoryChange::Updated { .. }));
    assert_eq!(t.get("a").unwrap().get("n"), Some(&CborValue::Integer(2.into())));
    assert_eq!(t.get("a").unwrap().get("id"), Some(&text("a")));
    assert_eq!(t.writes(), 2);
}

#[test]
fn identical_upsert_sends_nothing() {
    let t = MemoryStore::new().table("ticket");
    t.upsert("a", rec(&[("n", CborValue::Integer(1.into()))]));
    let mut rx = t.subscribe();
    t.upsert("a", rec(&[("n", CborValue::Integer(1.into()))]));
    assert!(rx.try_recv().is_err());
    assert_eq!(t.writes(), 1);
}

#[test]
fn patch_merges_and_reports_old_row() {
    let t = MemoryStore::new().table("ticket");
    t.upsert("a", rec(&[("status", text("Open")), ("n", CborValue::Integer(1.into()))]));
    let mut rx = t.subscribe();
    assert!(t.patch("a", &rec(&[("status", text("Closed"))])));
    let row = t.get("a").unwrap();
    assert_eq!(row.get("status"), Some(&text("Closed")));
    assert_eq!(row.get("n"), Some(&CborValue::Integer(1.into())));
    match rx.try_recv().unwrap() {
        MemoryChange::Updated { old, row, .. } => {
            assert_eq!(old.get("status"), Some(&text("Open")));
            assert_eq!(row.get("status"), Some(&text("Closed")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn noop_patch_sends_nothing() {
    let t = MemoryStore::new().table("ticket");
    t.upsert("a", rec(&[("status", text("Open"))]));
    let mut rx = t.subscribe();
    assert!(t.patch("a", &rec(&[("status", text("Open"))])));
    assert!(rx.try_recv().is_err());
    assert_eq!(t.writes(), 1);
}

#[test]
fn patch_and_delete_on_missing_id_return_false_silently() {
    let t = MemoryStore::new().table("ticket");
    let mut rx = t.subscribe();
    assert!(!t.patch("nope", &rec(&[("status", text("x"))])));
    assert!(!t.delete("nope"));
    assert!(rx.try_recv().is_err());
    assert_eq!(t.writes(), 0);
}

#[test]
fn delete_removes_and_reports_old_row() {
    let t = MemoryStore::new().table("ticket");
    t.upsert("a", rec(&[("status", text("Open"))]));
    let mut rx = t.subscribe();
    assert!(t.delete("a"));
    assert!(t.get("a").is_none());
    match rx.try_recv().unwrap() {
        MemoryChange::Deleted { id, old } => {
            assert_eq!(id, "a");
            assert_eq!(old.get("status"), Some(&text("Open")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn quiet_stores_without_events_but_counts_writes() {
    let t = MemoryStore::new().table("ticket");
    let mut rx = t.subscribe();
    t.set_quiet(true);
    t.insert(Record::new()).unwrap();
    assert!(rx.try_recv().is_err());
    assert_eq!((t.len(), t.writes()), (1, 1));
    t.set_quiet(false);
    t.insert(Record::new()).unwrap();
    assert!(rx.try_recv().is_ok());
}

#[test]
fn store_returns_the_same_table_by_name() {
    let s = MemoryStore::new();
    s.table("a").insert(Record::new()).unwrap();
    assert_eq!(s.table("a").len(), 1);
    assert_eq!(s.table_names(), vec!["a"]);
}
```

- [ ] **Step 3: Run the tests and check they fail**

Run: `cargo test -p vantage-memory --lib store 2>&1 | tail -15`
Expected: compile errors, because the store types don't exist yet.

- [ ] **Step 4: Implement**

`vantage-memory/src/store/events.rs`:

```rust
//! Change notifications a table broadcasts after each write that changed a row.

use crate::store::Row;

/// Channel backlog before a lagging subscriber gets `Lagged` and must re-list.
pub(crate) const EVENT_CAPACITY: usize = 4096;

#[derive(Clone, Debug)]
pub enum MemoryChange {
    Inserted { id: String, row: Row },
    Updated { id: String, row: Row, old: Row },
    Deleted { id: String, old: Row },
}

impl MemoryChange {
    pub fn id(&self) -> &str {
        match self {
            Self::Inserted { id, .. } | Self::Updated { id, .. } | Self::Deleted { id, .. } => id,
        }
    }
}
```

`vantage-memory/src/store/ids.rs`:

```rust
//! Per-table id generation: a counter, optionally prefixed, that skips ids
//! already taken by caller-supplied rows.

use std::sync::atomic::{AtomicU64, Ordering};

use ciborium::Value as CborValue;

pub(crate) struct IdGen {
    next: AtomicU64,
    prefix: Option<String>,
}

impl IdGen {
    pub fn new(prefix: Option<String>) -> Self {
        Self { next: AtomicU64::new(1), prefix }
    }

    /// The next id for which `taken` is false.
    pub fn next(&self, taken: impl Fn(&str) -> bool) -> String {
        loop {
            let n = self.next.fetch_add(1, Ordering::Relaxed);
            let id = match &self.prefix {
                Some(p) => format!("{p}{n}"),
                None => n.to_string(),
            };
            if !taken(&id) {
                return id;
            }
        }
    }
}

/// The id a row supplies in its id column: non-empty text or an integer.
pub(crate) fn supplied_id(value: Option<&CborValue>) -> Option<String> {
    match value {
        Some(CborValue::Text(s)) if !s.is_empty() => Some(s.clone()),
        Some(CborValue::Integer(i)) => Some(i128::from(*i).to_string()),
        _ => None,
    }
}
```

`vantage-memory/src/store/table.rs`:

```rust
//! One table: rows under a single lock, id generation and the change channel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use vantage_types::Record;

use super::events::{EVENT_CAPACITY, MemoryChange};
use super::ids::{IdGen, supplied_id};
use super::{Row, TableDef};

struct Rows {
    map: IndexMap<String, Row>,
}

pub struct MemoryTable {
    name: String,
    def: TableDef,
    rows: RwLock<Rows>,
    ids: IdGen,
    events: broadcast::Sender<MemoryChange>,
    quiet: AtomicBool,
    writes: AtomicU64,
}

impl MemoryTable {
    pub(crate) fn new(name: String, def: TableDef) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            ids: IdGen::new(def.id_prefix.clone()),
            rows: RwLock::new(Rows { map: IndexMap::new() }),
            name,
            def,
            events,
            quiet: AtomicBool::new(false),
            writes: AtomicU64::new(0),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id_column(&self) -> &str {
        &self.def.id_column
    }

    fn with_id(&self, mut record: Record<CborValue>, id: &str) -> Row {
        record.insert(self.def.id_column.clone(), CborValue::Text(id.to_string()));
        Arc::new(record)
    }

    fn changed(&self, change: MemoryChange) {
        self.writes.fetch_add(1, Ordering::Relaxed);
        if !self.quiet.load(Ordering::Relaxed) {
            let _ = self.events.send(change);
        }
    }

    /// Insert a new row. The id comes from the id column when supplied,
    /// else from the table's counter. An existing id is an error.
    pub fn insert(&self, record: Record<CborValue>) -> vantage_core::Result<String> {
        let mut rows = self.rows.write();
        let id = match supplied_id(record.get(&self.def.id_column)) {
            Some(id) if rows.map.contains_key(&id) => {
                return Err(vantage_core::error!("Row already exists", table = self.name, id = id));
            }
            Some(id) => id,
            None => self.ids.next(|candidate| rows.map.contains_key(candidate)),
        };
        let row = self.with_id(record, &id);
        rows.map.insert(id.clone(), row.clone());
        drop(rows);
        self.changed(MemoryChange::Inserted { id: id.clone(), row });
        Ok(id)
    }

    /// Insert or replace the row `id`. Replacing with an identical row is a no-op.
    pub fn upsert(&self, id: &str, record: Record<CborValue>) {
        let row = self.with_id(record, id);
        let mut rows = self.rows.write();
        let old = rows.map.insert(id.to_string(), row.clone());
        drop(rows);
        match old {
            None => self.changed(MemoryChange::Inserted { id: id.to_string(), row }),
            Some(old) if old == row => {}
            Some(old) => self.changed(MemoryChange::Updated { id: id.to_string(), row, old }),
        }
    }

    /// Merge `partial` into row `id`. `false` when the row is missing.
    pub fn patch(&self, id: &str, partial: &Record<CborValue>) -> bool {
        let mut rows = self.rows.write();
        let Some(old) = rows.map.get(id).cloned() else {
            return false;
        };
        let mut next = (*old).clone();
        for (k, v) in partial.iter() {
            next.insert(k.clone(), v.clone());
        }
        if next == *old {
            return true;
        }
        let row = Arc::new(next);
        rows.map.insert(id.to_string(), row.clone());
        drop(rows);
        self.changed(MemoryChange::Updated { id: id.to_string(), row, old });
        true
    }

    /// Remove row `id`. `false` when it is missing.
    pub fn delete(&self, id: &str) -> bool {
        let mut rows = self.rows.write();
        let Some(old) = rows.map.shift_remove(id) else {
            return false;
        };
        drop(rows);
        self.changed(MemoryChange::Deleted { id: id.to_string(), old });
        true
    }

    pub fn get(&self, id: &str) -> Option<Row> {
        self.rows.read().map.get(id).cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        self.rows.read().map.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.rows.read().map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Stop (or resume) broadcasting changes. Writes still apply and count.
    pub fn set_quiet(&self, quiet: bool) {
        self.quiet.store(quiet, Ordering::Relaxed);
    }

    pub fn is_quiet(&self) -> bool {
        self.quiet.load(Ordering::Relaxed)
    }

    /// Writes that changed a row since the table was created.
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::Relaxed)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MemoryChange> {
        self.events.subscribe()
    }
}
```

This needs `Record<CborValue>: PartialEq`. Check `vantage-types/src/record.rs`; if `Record` doesn't derive `PartialEq`, compare `as_inner()` maps instead (`old.as_inner() == row.as_inner()`).

`vantage-memory/src/store.rs`:

```rust
//! The store: named tables with a synchronous API. Sims and other threads
//! call it directly; the async trait layers wrap it.

mod events;
mod ids;
mod table;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;

use ciborium::Value as CborValue;
use parking_lot::RwLock;
use vantage_types::Record;

pub use events::MemoryChange;
pub use table::MemoryTable;

/// A stored row, shared between the table, readers and change events.
pub type Row = Arc<Record<CborValue>>;
pub type MemoryTableHandle = Arc<MemoryTable>;

/// How a table stores and identifies rows.
#[derive(Clone, Debug)]
pub struct TableDef {
    pub id_column: String,
    /// Columns with a hash index, used by `Eq` / `InSet` lookups.
    pub indexed: Vec<String>,
    /// Prefix for generated ids (`"T-"` gives `T-1`, `T-2`, …).
    pub id_prefix: Option<String>,
}

impl Default for TableDef {
    fn default() -> Self {
        Self { id_column: "id".into(), indexed: Vec::new(), id_prefix: None }
    }
}

#[derive(Clone, Default)]
pub struct MemoryStore {
    tables: Arc<RwLock<HashMap<String, MemoryTableHandle>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The table `name`, created with the default definition if missing.
    pub fn table(&self, name: &str) -> MemoryTableHandle {
        self.define(name, TableDef::default())
    }

    /// The table `name`, created with `def` if missing. An existing table
    /// keeps its original definition.
    pub fn define(&self, name: &str, def: TableDef) -> MemoryTableHandle {
        if let Some(t) = self.tables.read().get(name) {
            return t.clone();
        }
        self.tables
            .write()
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(MemoryTable::new(name.to_string(), def)))
            .clone()
    }

    pub fn table_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tables.read().keys().cloned().collect();
        names.sort();
        names
    }
}
```

- [ ] **Step 5: Run the tests and check they pass**

Run: `cargo test -p vantage-memory --lib store 2>&1 | tail -15`
Expected: all 12 tests pass.

- [ ] **Step 6: Commit**

```bash
cargo fmt -p vantage-memory
git add Cargo.toml Cargo.lock vantage-memory/Cargo.toml vantage-memory/src
git commit -m "vantage-memory: crate scaffold and the store core"
```

---

### Task 2: Type system (`AnyMemoryType`)

**Files:**
- Create: `vantage-memory/src/types.rs` and `vantage-memory/src/types/{value,numbers,string,bool,bytes}.rs`
- Modify: `vantage-memory/src/lib.rs` (add `pub mod types;` and `pub use types::{AnyMemoryType, MemoryType, MemoryTypeVariants};`)

**Interfaces:**
- Produces:
  - `AnyMemoryType`, `MemoryType`, `MemoryTypeVariants`, generated by `vantage_type_system!` with `type_trait: MemoryType, method_name: cbor, value_type: ciborium::Value, null_when: ciborium::Value::Null, type_variants: [Null, Bool, Int, Float, String, Bytes, Array, Map]`
  - `AnyMemoryType::untyped(ciborium::Value) -> Self`
  - `impl MemoryType for AnyMemoryType`, and `impl<T: MemoryType> MemoryType for Option<T>`
  - `From<i32|i64|u32|u64|f32|f64|bool|String|&str|Vec<u8>> for AnyMemoryType`
  - `impl Expressive<AnyMemoryType>` for those scalars
  - `TryFrom<AnyMemoryType>` for those scalars
  - `TryFrom<AnyMemoryType> for Record<AnyMemoryType>`

This task is a port: copy vantage-redb's `src/types/` module (traits.md §3, "Type system"). Rename `Redb` → `Memory` and `redb` → `memory` in every identifier, keep the behaviour identical, and drop `serial.rs` if it only serves redb storage encoding (check what it does; keep it only if the other files depend on it).

- [ ] **Step 1: Write the failing tests** in `vantage-memory/src/types/tests.rs` (registered in `types.rs` under `#[cfg(test)]`):

```rust
use super::*;

#[test]
fn scalars_round_trip() {
    let v = AnyMemoryType::from(42i64);
    assert_eq!(i64::try_from(v.clone()).unwrap(), 42);
    assert_eq!(v.value(), &ciborium::Value::Integer(42.into()));
    let s = AnyMemoryType::from("hi");
    assert_eq!(String::try_from(s).unwrap(), "hi");
    let b = AnyMemoryType::from(true);
    assert!(bool::try_from(b).unwrap());
}

#[test]
fn untyped_accepts_any_cbor_and_compares_by_value() {
    let a = AnyMemoryType::untyped(ciborium::Value::Integer(5.into()));
    let b = AnyMemoryType::from(5i64);
    assert_eq!(a, b);
    assert_eq!(a.type_variant(), None);
}

#[test]
fn option_none_is_null() {
    assert_eq!(None::<i64>.to_cbor(), ciborium::Value::Null);
}
```

- [ ] **Step 2: Run the tests and check they fail**

Run: `cargo test -p vantage-memory --lib types 2>&1 | tail -10`
Expected: compile errors, because there's no `types` module yet.

- [ ] **Step 3: Implement.** Port the files as described above. `types.rs` holds the `vantage_type_system!` invocation (traits.md §3), the `mod` lines and `pub use` lines.

- [ ] **Step 4: Run the tests and check they pass**

Run: `cargo test -p vantage-memory --lib types 2>&1 | tail -10`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt -p vantage-memory
git add vantage-memory/src
git commit -m "vantage-memory: AnyMemoryType type system"
```

---

### Task 3: Conditions, the evaluator and `Query`

**Files:**
- Create: `vantage-memory/src/eval.rs` (`Query`, `run`, `count`, `matches_all`)
- Create: `vantage-memory/src/eval/compare.rs` (`lookup`, `cmp_values`, `values_eq`, `text_of`, `like`)
- Create: `vantage-memory/src/eval/condition.rs` (`MemoryCondition`, `matches`, `resolve`)
- Create: `vantage-memory/src/eval/order.rs` (`sort_rows`)
- Test: `vantage-memory/src/eval/tests.rs`
- Modify: `vantage-memory/src/store/table.rs` (add `query` and `count`)
- Modify: `vantage-memory/src/lib.rs` (`pub mod eval; pub use eval::{MemoryCondition, Query};`)

**Interfaces:**
- Consumes: `Row`, `MemoryTable` and its private `rows` lock (Task 1), `AnyMemoryType` (Task 2).
- Produces:
  - `MemoryCondition` (`Clone`): variants `Cmp { path: String, op: FilterOp, value: CborValue }`, `Search(String)`, `And(Vec<Self>)`, `Or(Vec<Self>)`, `Not(Box<Self>)`, `Column(String)`, `Deferred(DeferredFn<AnyMemoryType>)`
  - `MemoryCondition::cmp(path: impl Into<String>, op: FilterOp, value: impl Into<CborValue>) -> Self`
  - `MemoryCondition::matches(&self, &Record<CborValue>) -> vantage_core::Result<bool>`
  - `async MemoryCondition::resolve(self) -> vantage_core::Result<Self>` (recursive)
  - `Query { conditions: Vec<MemoryCondition>, search: Option<String>, order: Vec<(String, vantage_vista::SortDirection)>, offset: usize, limit: Option<usize> }`, deriving `Clone, Default`
  - builders `Query::new()`, `.filter(c)`, `.search(s)`, `.order_by(path, dir)`, `.window(offset, limit)`
  - `async Query::resolve(self) -> Result<Query>`
  - `eval::matches_all(&Query, &Record<CborValue>) -> Result<bool>` (conditions AND search)
  - `MemoryTable::query(&self, &Query) -> Result<Vec<(String, Row)>>`
  - `MemoryTable::count(&self, &Query) -> Result<usize>`, which ignores order, offset and limit
  - `eval::compare::{lookup, cmp_values, values_eq, text_of, like}` as `pub`

- [ ] **Step 1: Write the failing tests** in `vantage-memory/src/eval/tests.rs`:

```rust
use ciborium::Value as CborValue;
use vantage_types::Record;
use vantage_vista::{FilterOp, SortDirection};

use super::*;
use crate::store::MemoryStore;

fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}
fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn rec(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}
fn yes(c: MemoryCondition, r: &Record<CborValue>) -> bool {
    c.matches(r).unwrap()
}

#[test]
fn ordered_ops_compare_numbers_across_int_and_float() {
    let r = rec(&[("n", int(5))]);
    assert!(yes(MemoryCondition::cmp("n", FilterOp::Gt, CborValue::Float(4.5)), &r));
    assert!(yes(MemoryCondition::cmp("n", FilterOp::Gte, int(5)), &r));
    assert!(yes(MemoryCondition::cmp("n", FilterOp::Lt, CborValue::Float(5.1)), &r));
    assert!(!yes(MemoryCondition::cmp("n", FilterOp::Lte, int(4)), &r));
}

#[test]
fn eq_int_matches_float() {
    let r = rec(&[("n", CborValue::Float(1.0))]);
    assert!(yes(MemoryCondition::cmp("n", FilterOp::Eq, int(1)), &r));
    assert!(yes(MemoryCondition::cmp("n", FilterOp::InSet, CborValue::Array(vec![int(3), int(1)])), &r));
}

#[test]
fn mixed_kinds_never_match_ordered_ops() {
    let r = rec(&[("n", text("5"))]);
    assert!(!yes(MemoryCondition::cmp("n", FilterOp::Gt, int(1)), &r));
    assert!(!yes(MemoryCondition::cmp("n", FilterOp::Lt, int(9)), &r));
}

#[test]
fn null_rules() {
    let r = rec(&[("a", CborValue::Null)]);
    let missing = rec(&[]);
    assert!(yes(MemoryCondition::cmp("a", FilterOp::Eq, CborValue::Null), &r));
    assert!(yes(MemoryCondition::cmp("a", FilterOp::Eq, CborValue::Null), &missing));
    assert!(yes(MemoryCondition::cmp("a", FilterOp::Ne, int(1)), &r));
    assert!(!yes(MemoryCondition::cmp("a", FilterOp::Ne, CborValue::Null), &r));
    assert!(yes(MemoryCondition::cmp("a", FilterOp::NotInSet, CborValue::Array(vec![int(1)])), &r));
    assert!(!yes(MemoryCondition::cmp("a", FilterOp::NotInSet, CborValue::Array(vec![CborValue::Null])), &r));
    assert!(!yes(MemoryCondition::cmp("a", FilterOp::Gt, int(0)), &r));
    assert!(!yes(MemoryCondition::cmp("a", FilterOp::Like, text("%")), &r));
}

#[test]
fn dotted_paths_read_nested_maps() {
    let addr = CborValue::Map(vec![(text("city"), text("Riga"))]);
    let r = rec(&[("address", addr)]);
    assert!(yes(MemoryCondition::cmp("address.city", FilterOp::Eq, text("Riga")), &r));
    assert!(yes(MemoryCondition::cmp("address.zip", FilterOp::Eq, CborValue::Null), &r));
}

#[test]
fn a_column_named_with_a_dot_wins_over_the_path() {
    let r = rec(&[("a.b", int(1))]);
    assert!(yes(MemoryCondition::cmp("a.b", FilterOp::Eq, int(1)), &r));
}

#[test]
fn like_is_case_insensitive_with_wildcards() {
    let r = rec(&[("name", text("Hello World"))]);
    assert!(yes(MemoryCondition::cmp("name", FilterOp::Like, text("hello%")), &r));
    assert!(yes(MemoryCondition::cmp("name", FilterOp::Like, text("%o_w%")), &r));
    assert!(!yes(MemoryCondition::cmp("name", FilterOp::Like, text("world")), &r));
    let n = rec(&[("n", int(12345))]);
    assert!(yes(MemoryCondition::cmp("n", FilterOp::Like, text("%234%")), &n));
}

#[test]
fn boolean_combinators() {
    let r = rec(&[("a", int(1)), ("b", int(2))]);
    let a1 = MemoryCondition::cmp("a", FilterOp::Eq, int(1));
    let b9 = MemoryCondition::cmp("b", FilterOp::Eq, int(9));
    assert!(!yes(MemoryCondition::And(vec![a1.clone(), b9.clone()]), &r));
    assert!(yes(MemoryCondition::Or(vec![a1.clone(), b9.clone()]), &r));
    assert!(yes(MemoryCondition::Not(Box::new(b9)), &r));
}

#[test]
fn search_checks_text_and_numbers_case_insensitively() {
    let r = rec(&[("name", text("Alice")), ("n", int(42))]);
    assert!(yes(MemoryCondition::Search("ALI".into()), &r));
    assert!(yes(MemoryCondition::Search("42".into()), &r));
    assert!(!yes(MemoryCondition::Search("bob".into()), &r));
}

#[test]
fn column_and_unresolved_deferred_are_errors_as_filters() {
    let r = rec(&[]);
    assert!(MemoryCondition::Column("a".into()).matches(&r).is_err());
}

#[tokio::test]
async fn deferred_resolves_to_in_set() {
    use vantage_expressions::traits::expressive::DeferredFn;
    let d = DeferredFn::from_fn(|| async {
        Ok::<_, vantage_core::VantageError>(crate::AnyMemoryType::untyped(CborValue::Array(vec![
            text("owner"),
            CborValue::Array(vec![text("u1"), text("u2")]),
        ])))
    });
    let c = MemoryCondition::And(vec![MemoryCondition::Deferred(d)]).resolve().await.unwrap();
    assert!(yes(c.clone(), &rec(&[("owner", text("u2"))])));
    assert!(!yes(c, &rec(&[("owner", text("u3"))])));
}

fn filled() -> crate::store::MemoryTableHandle {
    let t = MemoryStore::new().table("t");
    for (id, n, name) in [("a", 3, "Cy"), ("b", 1, "Al"), ("c", 2, "Bo"), ("d", 1, "Di")] {
        t.upsert(id, rec(&[("n", int(n)), ("name", text(name))]));
    }
    t
}

fn ids(rows: Vec<(String, crate::Row)>) -> Vec<String> {
    rows.into_iter().map(|(id, _)| id).collect()
}

#[test]
fn query_filters_orders_and_pages() {
    let t = filled();
    let q = Query::new()
        .filter(MemoryCondition::cmp("n", FilterOp::Lte, int(2)))
        .order_by("n", SortDirection::Ascending)
        .order_by("name", SortDirection::Descending);
    assert_eq!(ids(t.query(&q).unwrap()), ["d", "b", "c"]);
    assert_eq!(ids(t.query(&q.clone().window(1, Some(1))).unwrap()), ["b"]);
    assert_eq!(t.count(&q).unwrap(), 3);
}

#[test]
fn ordering_ties_keep_insertion_order_and_nulls_first() {
    let t = filled();
    t.upsert("e", rec(&[("name", text("Ed"))]));
    let q = Query::new().order_by("n", SortDirection::Ascending);
    assert_eq!(ids(t.query(&q).unwrap()), ["e", "b", "d", "c", "a"]);
    let q = Query::new().order_by("n", SortDirection::Descending);
    assert_eq!(ids(t.query(&q).unwrap()), ["a", "c", "b", "d", "e"]);
}

#[test]
fn offset_past_end_is_empty() {
    let t = filled();
    assert!(t.query(&Query::new().window(10, Some(5))).unwrap().is_empty());
    assert!(t.query(&Query::new().window(0, Some(0))).unwrap().is_empty());
}

#[test]
fn query_search_combines_with_conditions() {
    let t = filled();
    let q = Query::new().search("o").filter(MemoryCondition::cmp("n", FilterOp::Eq, int(2)));
    assert_eq!(ids(t.query(&q).unwrap()), ["c"]);
}
```

A note on the descending tie case: descending reverses the whole ordering, so `b` and `d` (both `n = 1`) come out as `b, d`, because the sort is stable over the reversed comparison. Implement descending as `cmp.reverse()` in a stable sort. If the test's expected order disagrees with a stable-sort implementation, the test is what's wrong: fix the expectation to what a stable sort over the reversed comparator produces, and note it in your report.

- [ ] **Step 2: Run the tests and check they fail**

Run: `cargo test -p vantage-memory --lib eval 2>&1 | tail -10`
Expected: compile errors.

- [ ] **Step 3: Implement `compare.rs`**

```rust
//! Cell lookup and comparison shared by conditions, search and ordering.

use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_types::Record;

/// A cell by column name, or by dotted path into nested maps. A column whose
/// name contains a dot wins over the path reading.
pub fn lookup<'a>(record: &'a Record<CborValue>, path: &str) -> Option<&'a CborValue> {
    if let Some(v) = record.get(path) {
        return Some(v);
    }
    let mut parts = path.split('.');
    let mut cur = record.get(parts.next()?)?;
    for part in parts {
        let CborValue::Map(entries) = cur else { return None };
        cur = entries
            .iter()
            .find(|(k, _)| matches!(k, CborValue::Text(t) if t == part))
            .map(|(_, v)| v)?;
    }
    Some(cur)
}

fn number(v: &CborValue) -> Option<f64> {
    match v {
        CborValue::Integer(i) => Some(i128::from(*i) as f64),
        CborValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// Order two non-null values of the same kind; numbers compare across int
/// and float. `None` for different kinds.
pub fn cmp_values(a: &CborValue, b: &CborValue) -> Option<Ordering> {
    match (a, b) {
        (CborValue::Integer(x), CborValue::Integer(y)) => Some(i128::from(*x).cmp(&i128::from(*y))),
        (CborValue::Text(x), CborValue::Text(y)) => Some(x.cmp(y)),
        (CborValue::Bool(x), CborValue::Bool(y)) => Some(x.cmp(y)),
        _ => number(a)?.partial_cmp(&number(b)?),
    }
}

/// Equality that treats `1` and `1.0` as equal; other values compare exactly.
pub fn values_eq(a: &CborValue, b: &CborValue) -> bool {
    match cmp_values(a, b) {
        Some(o) => o == Ordering::Equal,
        None => a == b,
    }
}

/// The text a cell offers to search and `Like`: text, or a number's decimal form.
pub fn text_of(v: &CborValue) -> Option<String> {
    match v {
        CborValue::Text(s) => Some(s.clone()),
        CborValue::Integer(i) => Some(i128::from(*i).to_string()),
        CborValue::Float(f) => Some(f.to_string()),
        _ => None,
    }
}

/// SQL-style `LIKE`, case-insensitive: `%` matches any run, `_` one character.
pub fn like(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '_' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '%' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '%')
}
```

- [ ] **Step 4: Implement `condition.rs`**

```rust
//! Filter conditions both layers compile to.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_expressions::traits::expressive::{DeferredFn, ExpressiveEnum};
use vantage_types::Record;
use vantage_vista::{FilterOp, operand_text};

use super::compare::{cmp_values, like, lookup, text_of, values_eq};
use crate::AnyMemoryType;

#[derive(Clone)]
pub enum MemoryCondition {
    /// `path op value`. For `InSet` / `NotInSet`, `value` is an array.
    Cmp { path: String, op: FilterOp, value: CborValue },
    /// Case-insensitive substring over every text or number cell.
    Search(String),
    And(Vec<MemoryCondition>),
    Or(Vec<MemoryCondition>),
    Not(Box<MemoryCondition>),
    /// A bare column reference, valid only as an order key.
    Column(String),
    /// Resolved before evaluation into `Cmp { op: InSet }`. The payload is
    /// a CBOR array `[Text(path), Array(values)]`.
    Deferred(DeferredFn<AnyMemoryType>),
}

impl MemoryCondition {
    pub fn cmp(path: impl Into<String>, op: FilterOp, value: impl Into<CborValue>) -> Self {
        Self::Cmp { path: path.into(), op, value: value.into() }
    }

    pub fn matches(&self, record: &Record<CborValue>) -> Result<bool> {
        Ok(match self {
            Self::Cmp { path, op, value } => cmp_matches(lookup(record, path), *op, value),
            Self::Search(text) => search_matches(record, text),
            Self::And(all) => {
                for c in all {
                    if !c.matches(record)? {
                        return Ok(false);
                    }
                }
                true
            }
            Self::Or(any) => {
                for c in any {
                    if c.matches(record)? {
                        return Ok(true);
                    }
                }
                false
            }
            Self::Not(inner) => !inner.matches(record)?,
            Self::Column(c) => return Err(error!("A column reference is not a filter", column = c)),
            Self::Deferred(_) => return Err(error!("Deferred condition used before resolve()")),
        })
    }

    /// Resolve every `Deferred` in the tree.
    pub fn resolve(self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Self>> + Send>> {
        Box::pin(async move {
            Ok(match self {
                Self::Deferred(d) => deferred_to_in_set(d).await?,
                Self::And(all) => Self::And(resolve_all(all).await?),
                Self::Or(any) => Self::Or(resolve_all(any).await?),
                Self::Not(inner) => Self::Not(Box::new(inner.resolve().await?)),
                other => other,
            })
        })
    }
}

async fn resolve_all(list: Vec<MemoryCondition>) -> Result<Vec<MemoryCondition>> {
    let mut out = Vec::with_capacity(list.len());
    for c in list {
        out.push(c.resolve().await?);
    }
    Ok(out)
}

async fn deferred_to_in_set(d: DeferredFn<AnyMemoryType>) -> Result<MemoryCondition> {
    let ExpressiveEnum::Scalar(any) = d.call().await? else {
        return Err(error!("Deferred condition produced a non-scalar"));
    };
    match any.into_value() {
        CborValue::Array(parts) if parts.len() == 2 => {
            let mut it = parts.into_iter();
            match (it.next(), it.next()) {
                (Some(CborValue::Text(path)), Some(values @ CborValue::Array(_))) => {
                    Ok(MemoryCondition::cmp(path, FilterOp::InSet, values))
                }
                _ => Err(error!("Deferred condition: expected [path, [values]]")),
            }
        }
        _ => Err(error!("Deferred condition: expected [path, [values]]")),
    }
}

fn in_set(cell: &CborValue, set: &CborValue) -> bool {
    match set {
        CborValue::Array(items) => items.iter().any(|v| values_eq(cell, v)),
        single => values_eq(cell, single),
    }
}

fn cmp_matches(cell: Option<&CborValue>, op: FilterOp, value: &CborValue) -> bool {
    let cell = cell.unwrap_or(&CborValue::Null);
    let null = matches!(cell, CborValue::Null);
    match op {
        FilterOp::Eq => values_eq(cell, value),
        FilterOp::Ne => !values_eq(cell, value),
        FilterOp::InSet => in_set(cell, value),
        FilterOp::NotInSet => !in_set(cell, value),
        FilterOp::Like => !null && text_of(cell).is_some_and(|t| like(&operand_text(value), &t)),
        ordered => !null && cmp_values(cell, value).is_some_and(|o| ordered.matches_ordering(o)),
    }
}

fn search_matches(record: &Record<CborValue>, text: &str) -> bool {
    let needle = text.to_lowercase();
    record.values().any(|v| text_of(v).is_some_and(|t| t.to_lowercase().contains(&needle)))
}
```

Check `Ne` against the null rules. A null cell with `Ne 1` gives `!values_eq(Null, 1)` = true, which is right. A null cell with `Ne Null` gives false, also right. A non-null cell with `Ne Null` gives true.

- [ ] **Step 5: Implement `order.rs` and `eval.rs`, and add `query`/`count` to `MemoryTable`**

`order.rs`:

```rust
//! Multi-key stable ordering. Nulls sort first ascending; descending
//! reverses the whole comparison.

use std::cmp::Ordering;

use ciborium::Value as CborValue;
use vantage_vista::SortDirection;

use super::compare::{cmp_values, lookup};
use crate::Row;

fn rank(v: Option<&CborValue>) -> u8 {
    match v {
        None | Some(CborValue::Null) => 0,
        Some(CborValue::Bool(_)) => 1,
        Some(CborValue::Integer(_) | CborValue::Float(_)) => 2,
        Some(CborValue::Text(_)) => 3,
        Some(_) => 4,
    }
}

fn cmp_cells(a: Option<&CborValue>, b: Option<&CborValue>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => cmp_values(x, y).unwrap_or_else(|| rank(a).cmp(&rank(b))),
        _ => rank(a).cmp(&rank(b)),
    }
}

pub fn sort_rows(rows: &mut [(String, Row)], order: &[(String, SortDirection)]) {
    if order.is_empty() {
        return;
    }
    rows.sort_by(|(_, a), (_, b)| {
        for (path, dir) in order {
            let o = cmp_cells(lookup(a, path), lookup(b, path));
            let o = match dir {
                SortDirection::Ascending => o,
                SortDirection::Descending => o.reverse(),
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    });
}
```

`eval.rs`:

```rust
//! Queries: the one pipeline both layers compile to.
//! Candidates → conditions → search → order → offset/limit.

pub mod compare;
mod condition;
mod order;
#[cfg(test)]
mod tests;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::SortDirection;

pub use condition::MemoryCondition;
pub use order::sort_rows;

#[derive(Clone, Default)]
pub struct Query {
    /// AND-ed together.
    pub conditions: Vec<MemoryCondition>,
    pub search: Option<String>,
    pub order: Vec<(String, SortDirection)>,
    pub offset: usize,
    pub limit: Option<usize>,
}

impl Query {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn filter(mut self, c: MemoryCondition) -> Self {
        self.conditions.push(c);
        self
    }
    pub fn search(mut self, text: impl Into<String>) -> Self {
        self.search = Some(text.into());
        self
    }
    pub fn order_by(mut self, path: impl Into<String>, dir: SortDirection) -> Self {
        self.order.push((path.into(), dir));
        self
    }
    pub fn window(mut self, offset: usize, limit: Option<usize>) -> Self {
        self.offset = offset;
        self.limit = limit;
        self
    }

    /// Resolve every `Deferred` condition.
    pub async fn resolve(mut self) -> Result<Self> {
        let mut out = Vec::with_capacity(self.conditions.len());
        for c in self.conditions {
            out.push(c.resolve().await?);
        }
        self.conditions = out;
        Ok(self)
    }
}

/// Whether a row passes every condition and the search.
pub fn matches_all(q: &Query, record: &Record<CborValue>) -> Result<bool> {
    for c in &q.conditions {
        if !c.matches(record)? {
            return Ok(false);
        }
    }
    Ok(match &q.search {
        Some(s) => MemoryCondition::Search(s.clone()).matches(record)?,
        None => true,
    })
}
```

In `store/table.rs`, add:

```rust
    /// Rows matching `q`, ordered and windowed.
    pub fn query(&self, q: &crate::eval::Query) -> vantage_core::Result<Vec<(String, Row)>> {
        let mut out = Vec::new();
        {
            let rows = self.rows.read();
            for (id, row) in rows.map.iter() {
                if crate::eval::matches_all(q, row)? {
                    out.push((id.clone(), row.clone()));
                }
            }
        }
        crate::eval::sort_rows(&mut out, &q.order);
        let end = q.limit.map_or(out.len(), |l| q.offset.saturating_add(l).min(out.len()));
        let start = q.offset.min(end);
        Ok(out.drain(start..end).collect())
    }

    /// How many rows match `q`'s conditions and search (order and window ignored).
    pub fn count(&self, q: &crate::eval::Query) -> vantage_core::Result<usize> {
        let rows = self.rows.read();
        let mut n = 0;
        for row in rows.map.values() {
            if crate::eval::matches_all(q, row)? {
                n += 1;
            }
        }
        Ok(n)
    }
```

(Task 4 replaces the full scan in these two methods with index-backed candidates.)

- [ ] **Step 6: Run the tests and check they pass**

Run: `cargo test -p vantage-memory --lib eval 2>&1 | tail -10`
Expected: all eval tests pass.

- [ ] **Step 7: Commit**

```bash
cargo fmt -p vantage-memory
git add vantage-memory/src
git commit -m "vantage-memory: conditions, evaluator and queries"
```

---

### Task 4: Hash indexes

**Files:**
- Create: `vantage-memory/src/store/index.rs`
- Modify: `vantage-memory/src/store/table.rs` (`Rows` gains `indexes`; write paths maintain them; `query`/`count` use candidates)
- Test: add to `vantage-memory/src/store/tests.rs`

**Interfaces:**
- Consumes: `TableDef.indexed`, `Rows`, `Query`, `MemoryCondition::Cmp` (Tasks 1 and 3).
- Produces:
  - `pub(crate) struct Indexes`, with `new(&[String])`, `add(&mut self, id: &str, row: &Record<CborValue>)`, `remove(&mut self, id: &str, row: &Record<CborValue>)`, `candidates(&self, q: &Query) -> Option<IndexSet<String>>`
  - the private `Rows` gains `indexes: Indexes`

- [ ] **Step 1: Write the failing tests** (append to `store/tests.rs`):

```rust
use crate::eval::{MemoryCondition, Query};
use vantage_vista::FilterOp;

fn indexed() -> MemoryTableHandle {
    let s = MemoryStore::new();
    s.define("t", TableDef { indexed: vec!["status".into(), "n".into()], ..TableDef::default() })
}

fn all_ids(t: &MemoryTableHandle, q: &Query) -> Vec<String> {
    t.query(q).unwrap().into_iter().map(|(id, _)| id).collect()
}

#[test]
fn indexed_eq_agrees_with_scan_through_writes() {
    let t = indexed();
    for (id, status) in [("a", "Open"), ("b", "Closed"), ("c", "Open")] {
        t.upsert(id, rec(&[("status", text(status))]));
    }
    t.patch("a", &rec(&[("status", text("Closed"))]));
    t.delete("c");
    t.upsert("d", rec(&[("status", text("Open"))]));
    let q = Query::new().filter(MemoryCondition::cmp("status", FilterOp::Eq, text("Closed")));
    assert_eq!(all_ids(&t, &q), ["a", "b"]);
    let q = Query::new().filter(MemoryCondition::cmp("status", FilterOp::Eq, text("Open")));
    assert_eq!(all_ids(&t, &q), ["d"]);
}

#[test]
fn indexed_in_set_keeps_insertion_order() {
    let t = indexed();
    for (id, status) in [("a", "x"), ("b", "y"), ("c", "z"), ("d", "x")] {
        t.upsert(id, rec(&[("status", text(status))]));
    }
    let set = CborValue::Array(vec![text("x"), text("z")]);
    let q = Query::new().filter(MemoryCondition::cmp("status", FilterOp::InSet, set));
    assert_eq!(all_ids(&t, &q), ["a", "c", "d"]);
}

#[test]
fn index_matches_int_and_integral_float() {
    let t = indexed();
    t.upsert("a", rec(&[("n", CborValue::Float(1.0))]));
    t.upsert("b", rec(&[("n", CborValue::Integer(2.into()))]));
    let q = Query::new().filter(MemoryCondition::cmp("n", FilterOp::Eq, CborValue::Integer(1.into())));
    assert_eq!(all_ids(&t, &q), ["a"]);
}

#[test]
fn unkeyable_values_still_found_via_index_path() {
    let t = indexed();
    let map = CborValue::Map(vec![(text("k"), text("v"))]);
    t.upsert("a", rec(&[("status", map.clone())]));
    let q = Query::new().filter(MemoryCondition::cmp("status", FilterOp::Eq, map));
    assert_eq!(all_ids(&t, &q), ["a"]);
}
```

- [ ] **Step 2: Run the tests and check they fail**

Run: `cargo test -p vantage-memory --lib store 2>&1 | tail -10`
Expected: the new tests fail, either because the index isn't used (they may still pass by scanning) or because `Indexes` doesn't compile. `indexed_eq_agrees_with_scan_through_writes` and `index_matches_int_and_integral_float` guard correctness either way. To confirm the index path is actually taken, also add this unit test inside `index.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::{MemoryCondition, Query};
    use vantage_vista::FilterOp;

    #[test]
    fn candidates_come_from_the_index() {
        let mut ix = Indexes::new(&["status".to_string()]);
        let row: Record<CborValue> = [("status".to_string(), CborValue::Text("Open".into()))].into_iter().collect();
        ix.add("a", &row);
        let q = Query::new().filter(MemoryCondition::cmp("status", FilterOp::Eq, CborValue::Text("Open".into())));
        let c = ix.candidates(&q).unwrap();
        assert_eq!(c.into_iter().collect::<Vec<_>>(), vec!["a".to_string()]);
        let q = Query::new().filter(MemoryCondition::cmp("other", FilterOp::Eq, CborValue::Null));
        assert!(ix.candidates(&q).is_none());
    }
}
```

- [ ] **Step 3: Implement `index.rs`**

```rust
//! Hash indexes on declared columns. `Eq` / `InSet` conditions AND-ed at the
//! top level of a query take their candidate rows from here.

use std::collections::HashMap;

use ciborium::Value as CborValue;
use indexmap::IndexSet;
use vantage_types::Record;
use vantage_vista::FilterOp;

use crate::eval::compare::lookup;
use crate::eval::{MemoryCondition, Query};

/// A hashable form of a cell. Integers and integral floats share a key so
/// `1` and `1.0` meet. Maps, arrays and bytes have no key.
fn key(v: &CborValue) -> Option<String> {
    match v {
        CborValue::Null => Some("null".into()),
        CborValue::Bool(b) => Some(format!("b:{b}")),
        CborValue::Integer(i) => Some(format!("n:{}", i128::from(*i))),
        CborValue::Float(f) if f.fract() == 0.0 && f.is_finite() && f.abs() < 1e30 => Some(format!("n:{}", *f as i128)),
        CborValue::Float(f) => Some(format!("f:{}", f.to_bits())),
        CborValue::Text(s) => Some(format!("s:{s}")),
        _ => None,
    }
}

#[derive(Default)]
struct HashIndex {
    by_key: HashMap<String, IndexSet<String>>,
    /// Rows whose cell has no key; always candidates.
    unkeyed: IndexSet<String>,
}

pub(crate) struct Indexes {
    columns: HashMap<String, HashIndex>,
}

impl Indexes {
    pub fn new(columns: &[String]) -> Self {
        Self { columns: columns.iter().map(|c| (c.clone(), HashIndex::default())).collect() }
    }

    pub fn add(&mut self, id: &str, row: &Record<CborValue>) {
        for (col, ix) in self.columns.iter_mut() {
            let cell = lookup(row, col).unwrap_or(&CborValue::Null);
            match key(cell) {
                Some(k) => {
                    ix.by_key.entry(k).or_default().insert(id.to_string());
                }
                None => {
                    ix.unkeyed.insert(id.to_string());
                }
            }
        }
    }

    pub fn remove(&mut self, id: &str, row: &Record<CborValue>) {
        for (col, ix) in self.columns.iter_mut() {
            let cell = lookup(row, col).unwrap_or(&CborValue::Null);
            match key(cell) {
                Some(k) => {
                    if let Some(set) = ix.by_key.get_mut(&k) {
                        set.shift_remove(id);
                        if set.is_empty() {
                            ix.by_key.remove(&k);
                        }
                    }
                }
                None => {
                    ix.unkeyed.shift_remove(id);
                }
            }
        }
    }

    /// Candidate ids from the first top-level `Eq` / `InSet` condition on an
    /// indexed column, or `None` when no index applies. The caller still
    /// evaluates every condition on the candidates.
    pub fn candidates(&self, q: &Query) -> Option<IndexSet<String>> {
        q.conditions.iter().find_map(|c| {
            let MemoryCondition::Cmp { path, op, value } = c else { return None };
            let ix = self.columns.get(path)?;
            let values: Vec<&CborValue> = match (op, value) {
                (FilterOp::Eq, v) => vec![v],
                (FilterOp::InSet, CborValue::Array(items)) => items.iter().collect(),
                _ => return None,
            };
            let mut out: IndexSet<String> = ix.unkeyed.clone();
            for v in values {
                let k = key(v)?;
                if let Some(set) = ix.by_key.get(&k) {
                    out.extend(set.iter().cloned());
                }
            }
            Some(out)
        })
    }
}
```

`key(v)?` inside the loop returns `None`, meaning "no index", when the queried value is itself unkeyable, such as a map. That falls back to a scan, which is correct.

- [ ] **Step 4: Wire the indexes into `table.rs`**

- `Rows { map: IndexMap<String, Row>, indexes: Indexes }`. In `MemoryTable::new`, build it with `Indexes::new(&def.indexed)`.
- `insert` and `upsert` (new row): call `rows.indexes.add(&id, &row)` after inserting into `map`.
- `upsert` (replacing a row) and `patch`: call `rows.indexes.remove(id, &old)`, then `rows.indexes.add(id, &row)`, while still holding the write lock. Skip both when nothing changed.
- `delete`: call `rows.indexes.remove(id, &old)`.
- `query` and `count`: get candidates first. With `Some(ids)`, iterate the candidate ids, sorted by their position in `map` (`rows.map.get_index_of(id)`) so insertion order is kept. Skip ids no longer present. With `None`, scan `map` as before. Put the shared logic in one private helper, `fn matching(&self, q) -> Result<Vec<(String, Row)>>`, used by both methods; `count` just returns the length.

- [ ] **Step 5: Run the tests and check they pass**

Run: `cargo test -p vantage-memory --lib 2>&1 | tail -10`
Expected: all tests pass, old and new.

- [ ] **Step 6: Commit**

```bash
cargo fmt -p vantage-memory
git add vantage-memory/src
git commit -m "vantage-memory: hash indexes on declared columns"
```

---

### Task 5: Seed and dump

**Files:**
- Create: `vantage-memory/src/seed.rs`
- Modify: `vantage-memory/src/lib.rs` (`pub mod seed;`)
- Test: in `seed.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `MemoryTable::{upsert, insert, query, id_column}`, `Query`.
- Produces:
  - `seed::load(table: &MemoryTable, rows: impl IntoIterator<Item = serde_json::Value>) -> vantage_core::Result<usize>`
  - `seed::load_file(table: &MemoryTable, path: &Path) -> vantage_core::Result<usize>`, which reads `.json`, or `.yaml`/`.yml` via serde_yaml_ng
  - `seed::dump(table: &MemoryTable) -> Vec<serde_json::Value>`

Rules:
- Each value must be a JSON object; anything else is an error naming its position.
- An object with an id upserts. An object without one inserts, with a generated id.
- JSON ↔ CBOR conversion goes through `serde_json::Value` → `ciborium::Value`, using `ciborium::Value::serialized(&json)`, and back through `value.deserialized::<serde_json::Value>()`.
- `dump` returns rows in insertion order.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStore;
    use serde_json::json;

    #[test]
    fn load_upserts_by_id_and_inserts_without() {
        let t = MemoryStore::new().table("t");
        let n = load(&t, vec![json!({"id": "a", "n": 1}), json!({"n": 2})]).unwrap();
        assert_eq!(n, 2);
        assert_eq!(t.ids(), vec!["a", "1"]);
        load(&t, vec![json!({"id": "a", "n": 9})]).unwrap();
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn dump_round_trips_in_insertion_order() {
        let t = MemoryStore::new().table("t");
        let rows = vec![json!({"id": "b", "tags": ["x"], "m": {"k": 1.5}}), json!({"id": "a", "ok": true})];
        load(&t, rows.clone()).unwrap();
        assert_eq!(dump(&t), rows);
    }

    #[test]
    fn non_objects_are_rejected_with_position() {
        let t = MemoryStore::new().table("t");
        let err = load(&t, vec![json!({"id": "a"}), json!(3)]).unwrap_err().to_string();
        assert!(err.contains('1'), "{err}");
    }

    #[test]
    fn load_file_reads_yaml_and_json() {
        let dir = tempfile::tempdir().unwrap();
        let y = dir.path().join("rows.yaml");
        std::fs::write(&y, "- { id: a, n: 1 }\n- { id: b, n: 2 }\n").unwrap();
        let j = dir.path().join("rows.json");
        std::fs::write(&j, r#"[{"id": "c"}]"#).unwrap();
        let t = MemoryStore::new().table("t");
        assert_eq!(load_file(&t, &y).unwrap(), 2);
        assert_eq!(load_file(&t, &j).unwrap(), 1);
        assert_eq!(t.ids(), vec!["a", "b", "c"]);
    }
}
```

The round-trip test's field order must match. If `Record` preserves insertion order, which it does since it is IndexMap-based, the dump yields `id` first only when the loaded object had `id` first. The test objects above put `id` first on purpose.

- [ ] **Step 2: Run the tests and check they fail**

Run: `cargo test -p vantage-memory --lib seed 2>&1 | tail -10`
Expected: compile errors.

- [ ] **Step 3: Implement `seed.rs`**, following the rules above. Keep it under 100 LOC. Errors use `vantage_core::error!("Seed row is not an object", position = i)` and `error!("Cannot read seed file", path = ..., detail = e.to_string())`. Check how the `error!` macro accepts key-value pairs by reading one existing use, e.g. `grep -rn "error!(" vantage-csv/src | head`.

- [ ] **Step 4: Run the tests and check they pass**

Run: `cargo test -p vantage-memory --lib seed 2>&1 | tail -10`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt -p vantage-memory
git add vantage-memory/src
git commit -m "vantage-memory: seed load and dump"
```

---

### Task 6: Typed layer: `MemoryDB` as a `TableSource`

**Files:**
- Create: `vantage-memory/src/typed.rs` (`MemoryDB`, `DataSource`, `ExprDataSource`)
- Create: `vantage-memory/src/typed/operation.rs` (`MemoryOperation`)
- Create: `vantage-memory/src/typed/table_source.rs` (the `TableSource` impl)
- Create: `vantage-memory/src/typed/convert.rs` (between `Record<AnyMemoryType>` and `Record<CborValue>`; table → `Query`)
- Test: `vantage-memory/tests/typed.rs`
- Modify: `vantage-memory/src/lib.rs` (`pub mod typed; pub use typed::{MemoryDB, operation::MemoryOperation};` plus a `prelude` module mirroring redb's)

**Interfaces:**
- Consumes: `MemoryStore`, `MemoryTable` API, `Query`, `MemoryCondition` (+ `resolve`), `sort_rows`, `AnyMemoryType`.
- Produces:
  - `MemoryDB`: `Clone`, `new()`, `from_store(MemoryStore)`, `store(&self) -> &MemoryStore`
  - `TableSource for MemoryDB`: `Column<T> = vantage_table::column::core::Column<T>`, `AnyType = Value = AnyMemoryType`, `Id = String`, `Condition = MemoryCondition`, `Source = String`
  - `MemoryOperation<T>` (blanket over `Expressive<T>`): `eq ne gt gte lt lte` (value: `impl Into<AnyMemoryType>`), `in_`/`not_in` (iterator of `Into<AnyMemoryType>`), `like(impl Into<String>)`, `ascending()`/`descending()` → `vantage_table::sorting::OrderBy<MemoryCondition>`
  - `typed::convert::{to_cbor_record, from_cbor_record}` (pub(crate))
  - `async typed::convert::table_query<E>(&Table<MemoryDB, E>) -> Result<Query>`, which reads conditions, orders and pagination and resolves deferred conditions

Implementation guide. The signatures are in traits.md §2, and redb's implementation of each method is in §3. Follow redb's shape. Where they differ:

| Method | Behaviour |
|---|---|
| `create_column` / `to_any_column` / `convert_any_column` / `expr` | exactly as redb |
| `search_table_condition` | `MemoryCondition::Search(search_value.to_string())` (real search, not redb's unsupported stub) |
| `eq_condition(field, value: &str)` | `Cmp Eq` with `CborValue::Text(value)` |
| `eq_value_condition` | `Cmp Eq` with `value.into_value()` |
| `list_table_values` | `table_query(table)` → `store.table(table.table_name()).query(&q)` → map rows `from_cbor_record` into an `IndexMap<String, Record<AnyMemoryType>>` |
| `get_table_value` | `get(id)`, and also check the row passes the table's conditions (a conditioned table only sees its subset) |
| `get_table_some_value` | the first row of `query` with the window limit set to 1 |
| `get_table_count` | `count(&q)` as `i64` |
| `get_table_sum` / `max` / `min` | over `query` rows (without a window), reading the column. Sum: integers stay integer until a float appears, non-numeric cells are skipped, and an empty table gives `Integer(0)`. Max/min use `eval::compare::cmp_values` over non-null cells, and an empty table gives `Null`. Returns `AnyMemoryType::untyped(..)` |
| `insert_table_value(id, record)` | convert to CBOR and put the id into the id column. Errors if the id exists; otherwise `upsert`. Returns the stored record |
| `replace_table_value` | errors if missing; otherwise `upsert`. Returns the stored record |
| `patch_table_value` | `patch`; `false` → error `"Row not found"`. Returns the merged record |
| `delete_table_value` | `delete`; `false` → error `"Row not found"` |
| `delete_table_all_values` | deletes only the rows matching `table_query` (window ignored) |
| `insert_table_return_id_value` | `table.insert(cbor_record)` and return its id |
| `related_in_condition(target_field, source_table, source_column)` | `MemoryCondition::Deferred(DeferredFn::from_fn(..))`. The closure runs `list_table_values` on a clone of `source_table` and collects `source_column` values (or the row ids when `source_column` is the source's id column), producing `AnyMemoryType::untyped(Array([Text(target_field), Array(values)]))` |
| `column_table_values_expr` | copy redb's shape: a deferred expression that loads the filtered rows and returns one column's values as a CBOR array |
| `supports_traversal` | leave the default (`false`) |

Also:
- **`table_query`:**
  - AND together `table.conditions()`, resolve deferred conditions, and map `table.orders()`.
  - Each order key must be `MemoryCondition::Column(path)`; any other condition kind gives the error `"Order key must be a column"`. Map `vantage_table::sorting::SortDirection` to the Vista's `SortDirection`.
  - Pagination: `table.pagination()` → `offset = skip()`, `limit = Some(limit())` when present (check the sign conventions in `vantage-table/src/pagination.rs`; they are i64).
- **The id column** comes from `table.id_field().map(|c| c.name())`, falling back to the store table's `id_column()`. When they differ, the table's id field wins: create the store table with `define(name, TableDef { id_column, .. })` the first time a typed table touches it.
- **Invariants:** the shared vantage-table path applies them. Check `vantage-table/src/table/sets/invariants.rs` and a redb write method to see whether the backend or the table layer calls `enforce_invariants`. If it's the backend, call it the same way redb does.

- [ ] **Step 1: Write the failing tests** in `vantage-memory/tests/typed.rs`:

```rust
use vantage_memory::prelude::*;
use vantage_memory::MemoryDB;
use vantage_table::prelude::*;
use vantage_types::{EmptyEntity, Record};

fn products(db: &MemoryDB) -> Table<MemoryDB, EmptyEntity> {
    Table::new("product", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<String>("category")
}

async fn seeded() -> MemoryDB {
    let db = MemoryDB::new();
    let t = products(&db);
    for (id, name, price, cat) in [("p1", "Tea", 3, "drink"), ("p2", "Cake", 5, "food"), ("p3", "Coffee", 4, "drink")] {
        let r: Record<AnyMemoryType> = [
            ("name".to_string(), AnyMemoryType::from(name)),
            ("price".to_string(), AnyMemoryType::from(price as i64)),
            ("category".to_string(), AnyMemoryType::from(cat)),
        ]
        .into_iter()
        .collect();
        t.data_source().insert_table_value(&t, &id.to_string(), &r).await.unwrap();
    }
    db
}

#[tokio::test]
async fn conditions_filter_list_and_count() {
    let db = seeded().await;
    let mut t = products(&db);
    t.add_condition(t["price"].gt(3));
    let rows = t.data_source().list_table_values(&t).await.unwrap();
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), ["p2", "p3"]);
    assert_eq!(t.data_source().get_table_count(&t).await.unwrap(), 2);
}

#[tokio::test]
async fn order_and_pagination() {
    let db = seeded().await;
    let mut t = products(&db);
    t.add_order(t["price"].descending());
    t.set_pagination(Some(vantage_table::pagination::Pagination::window(1, 1)));
    let rows = t.data_source().list_table_values(&t).await.unwrap();
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), ["p3"]);
}

#[tokio::test]
async fn aggregates_respect_conditions() {
    let db = seeded().await;
    let mut t = products(&db);
    t.add_condition(t["category"].eq("drink"));
    let price = t["price"].clone();
    let sum = t.data_source().get_table_sum(&t, &price).await.unwrap();
    let max = t.data_source().get_table_max(&t, &price).await.unwrap();
    let min = t.data_source().get_table_min(&t, &price).await.unwrap();
    assert_eq!(i64::try_from(sum).unwrap(), 7);
    assert_eq!(i64::try_from(max).unwrap(), 4);
    assert_eq!(i64::try_from(min).unwrap(), 3);
}

#[tokio::test]
async fn crud_round_trip() {
    let db = seeded().await;
    let t = products(&db);
    let ds = t.data_source();
    let id = ds.insert_table_return_id_value(&t, &Record::new()).await.unwrap();
    assert_eq!(id, "1");
    let patch: Record<AnyMemoryType> = [("name".to_string(), AnyMemoryType::from("Scone"))].into_iter().collect();
    ds.patch_table_value(&t, &id, &patch).await.unwrap();
    let got = ds.get_table_value(&t, &id).await.unwrap().unwrap();
    assert_eq!(String::try_from(got["name"].clone()).unwrap(), "Scone");
    assert!(ds.insert_table_value(&t, &"p1".to_string(), &Record::new()).await.is_err());
    ds.delete_table_value(&t, &id).await.unwrap();
    assert!(ds.delete_table_value(&t, &id).await.is_err());
    assert!(ds.patch_table_value(&t, &id, &patch).await.is_err());
}

#[tokio::test]
async fn delete_all_honours_conditions() {
    let db = seeded().await;
    let mut t = products(&db);
    t.add_condition(t["category"].eq("drink"));
    t.data_source().delete_table_all_values(&t).await.unwrap();
    assert_eq!(db.store().table("product").ids(), vec!["p2"]);
}

#[tokio::test]
async fn search_matches_text_cells() {
    let db = seeded().await;
    let mut t = products(&db);
    let c = t.data_source().search_table_condition(&t, "cof");
    t.add_condition(c);
    let rows = t.data_source().list_table_values(&t).await.unwrap();
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), ["p3"]);
}

#[tokio::test]
async fn related_in_condition_narrows_by_source_rows() {
    let db = seeded().await;
    let orders = db.store().table("order");
    orders.upsert("o1", [("product".to_string(), ciborium::Value::Text("p2".into()))].into_iter().collect());
    let mut src = products(&db);
    src.add_condition(src["category"].eq("food"));
    let mut o = Table::<MemoryDB, EmptyEntity>::new("order", db.clone())
        .with_id_column("id")
        .with_column_of::<String>("product");
    let c = db.related_in_condition("product", &src, "id");
    o.add_condition(c);
    let rows = o.data_source().list_table_values(&o).await.unwrap();
    assert_eq!(rows.keys().cloned().collect::<Vec<_>>(), ["o1"]);
}
```

The builder and accessor names used here (`Table::new`, `with_id_column`, `with_column_of`, `add_condition`, `add_order`, `set_pagination`, `data_source()`, indexing `t["price"]`, `vantage_table::prelude`) are expected to exist in vantage-table. Check each against vantage-table's own tests (`grep -rn "with_column_of\|with_id_column\|set_pagination" vantage-table/tests vantage-redb/tests | head`). If a name differs, use the real one and record the difference in your report; don't change vantage-table.

- [ ] **Step 2: Run the tests and check they fail**

Run: `cargo test -p vantage-memory --test typed 2>&1 | tail -15`
Expected: compile errors.

- [ ] **Step 3: Implement** `typed.rs`, `operation.rs`, `convert.rs` and `table_source.rs`, following the guide above. `MemoryDB::execute` and `defer` copy redb's `ExprDataSource` impl (traits.md §3) with the types renamed.

- [ ] **Step 4: Run the tests and check they pass**

Run: `cargo test -p vantage-memory --test typed 2>&1 | tail -15`
Expected: 7 tests pass.

Run: `cargo test -p vantage-memory 2>&1 | tail -5`
Expected: everything passes.

- [ ] **Step 5: Commit**

```bash
cargo fmt -p vantage-memory
git add vantage-memory/src vantage-memory/tests
git commit -m "vantage-memory: typed TableSource layer"
```

---

### Task 7: Vista layer: `MemoryTableShell`, references and the factory

**Files:**
- Create: `vantage-memory/src/vista.rs` (`MemoryTableShell` state, constructor, capabilities)
- Create: `vantage-memory/src/vista/shell.rs` (the `impl TableShell`, without watch)
- Create: `vantage-memory/src/vista/refs.rs` (`get_ref`, `get_ref_target`, `Catalog`)
- Create: `vantage-memory/src/vista/factory.rs` (`MemoryVistaFactory`, `MemoryTableExtras`, `MemoryBlock`, `MemoryVistaSpec`)
- Test: `vantage-memory/tests/vista.rs`
- Modify: `vantage-memory/src/lib.rs` (`pub mod vista; pub use vista::{MemoryTableShell, factory::{MemoryVistaFactory, MemoryVistaSpec}};`)

**Interfaces:**
- Consumes: the store, `Query`, `MemoryCondition`, `sort_rows`, seed (Tasks 1–5).
- Produces:
  - `MemoryTableShell::new(table: MemoryTableHandle, metadata: VistaMetadata, catalog: Catalog) -> Self`
  - `MemoryTableShell::capabilities_for_memory() -> VistaCapabilities`
  - `pub(crate) fn MemoryTableShell::query(&self) -> &Query` (Task 8 uses it)
  - `pub(crate) fn MemoryTableShell::table(&self) -> &MemoryTableHandle`
  - `Catalog` (`Clone`): holds the `MemoryStore` plus a shared `Arc<RwLock<HashMap<String, VistaMetadata>>>`, with `Catalog::new(store: MemoryStore)`, `store(&self) -> &MemoryStore`, `register(name, VistaMetadata)` and `get(name) -> Option<VistaMetadata>`
  - `MemoryVistaFactory::new(store: MemoryStore) -> Self` and `store(&self) -> &MemoryStore`
  - `impl VistaFactory` with `TableExtras = MemoryTableExtras { memory: MemoryBlock { indexed: Vec<String>, seed: Option<PathBuf> } }` (both `Default`, `deny_unknown_fields`, `#[serde(default)]` on the `memory` field so it can be omitted), `ColumnExtras = NoExtras`, `ReferenceExtras = NoExtras`
  - `pub type MemoryVistaSpec = VistaSpec<MemoryTableExtras, NoExtras, NoExtras>`

Behaviour:

| Method | Behaviour |
|---|---|
| `columns` / `references` / `id_column` | from `metadata`; the id column falls back to the store table's `id_column()` |
| `list_vista_values` | `table.query(&self.query)` → `IndexMap<String, Record<CborValue>>`, cloning each `Row` |
| `get_vista_value` | `table.get(id)`, filtered by `eval::matches_all(&self.query, row)` |
| `get_vista_some_value` | the first row of the query with the limit set to 1 |
| `get_vista_count` | `table.count(&self.query)` |
| `insert_vista_value(id, rec)` | error if the id exists; else `upsert`; return the stored row |
| `replace_vista_value` | error if missing; else `upsert` |
| `patch_vista_value` | `patch`; `false` → `"Row not found"`; return the merged row |
| `delete_vista_value` | `delete`; `false` → `"Row not found"` |
| `delete_vista_all_values` | delete every row matching the query (window ignored) |
| `insert_vista_return_id_value` | `table.insert(rec)` |
| `import_vista_values(records)` | `upsert` each record; return the count |
| `add_eq_condition(f, v)` | push `Cmp Eq` |
| `add_op_condition(f, op, v)` | push `Cmp op` |
| `add_order(f, dir)` | **replace** the order with the single key `(f, dir)`; the trait's doc calls this replace semantics (see vantage-table's `clear_orders` doc). `clear_orders` empties it |
| `add_search` / `clear_search` | set / unset `query.search` |
| `set_page_size(n)` | store `n` |
| `fetch_page(p)` | 1-based; `p == 0` or no page size is an error. The window is `offset = (p-1)*size`, `limit = size`, applied to a clone of the query |
| `fetch_window(offset, limit)` | on a clone of the query |
| `fetch_window_counted` | the window plus `Some(count)` |
| `clone_shell` | same table and catalog; the query, metadata and page size are cloned |
| `get_ref(relation, row)` | see below |
| `get_ref_target(relation)` | a Vista over the target table, with no narrowing |
| `driver_name` | `"memory"` |
| `preview_query` | JSON `{ "driver": "memory", "table": .., "conditions": [<human-readable>], "search": .., "order": [..] }`. Write a small `describe(&MemoryCondition) -> String` for the conditions; `Deferred` shows as `"<deferred>"` |
| `capabilities` | count, insert, update, delete, import, order, search, filter_operators, set_page_size, fetch_page, fetch_window, traverse_to_record, traverse_to_set, subscribe, all `true`; everything else `false` |

Notes:
- **`can_subscribe` is set now,** but Task 8 implements `watch_vista`. Until then the trait default errors; Task 8's tests cover it.
- **`get_ref`:**
  - Look up `metadata.references[relation]`. The target shell is `MemoryTableShell::new(store.table(&reference.target), catalog.get(target).unwrap_or(id-only metadata), catalog)`, so the shell must also hold its `MemoryStore`: pass it in `new` or keep it inside `Catalog`. Choose `Catalog { store, metadata }` so `new` stays three arguments.
  - Narrowing: for `ReferenceKind::HasMany`, `target[foreign_key] == row[this id column]`. For `HasOne`, `target[target id column] == row[foreign_key]`.
  - Confirm this against how `vantage-sql/src/sqlite/vista/source.rs` implements `get_ref` for each kind. If SQLite reads it the other way round, follow SQLite and note it.
- **`build_from_spec`:**
  - Build `VistaMetadata` from the spec's columns (with flags), references and id column. Read `vantage-vista/src/spec.rs` for `VistaSpec`'s field names.
  - `store.define(name, TableDef { id_column, indexed: extras.memory.indexed, id_prefix: None })`.
  - `seed::load_file` if `seed` is set.
  - `catalog.register(name, metadata.clone())`.
  - Return `Vista::new(name, Box::new(shell))`.

- [ ] **Step 1: Write the failing tests** in `vantage-memory/tests/vista.rs`:

```rust
use ciborium::Value as CborValue;
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_vista::{FilterOp, SortDirection, VistaFactory};

const PRODUCT: &str = r#"
name: product
id_column: id
columns:
  id: { type: string, flags: [id] }
  name: { type: string, flags: [title, searchable] }
  price: { type: int }
  category: { type: string }
memory:
  indexed: [category]
"#;

const ORDER: &str = r#"
name: order
id_column: id
columns:
  id: { type: string, flags: [id] }
  product: { type: string }
  qty: { type: int }
references:
  product: { target: product, kind: has_one, foreign_key: product }
"#;

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn int(i: i64) -> CborValue {
    CborValue::Integer(i.into())
}

fn setup() -> (MemoryStore, MemoryVistaFactory) {
    let store = MemoryStore::new();
    let f = MemoryVistaFactory::new(store.clone());
    let p = store.table("product");
    for (id, name, price, cat) in [("p1", "Tea", 3, "drink"), ("p2", "Cake", 5, "food"), ("p3", "Coffee", 4, "drink")] {
        p.upsert(id, [("name".to_string(), text(name)), ("price".to_string(), int(price)), ("category".to_string(), text(cat))].into_iter().collect());
    }
    (store, f)
}

async fn ids(v: &vantage_vista::Vista) -> Vec<String> {
    v.list_values().await.unwrap().keys().cloned().collect()
}

#[tokio::test]
async fn capabilities_are_advertised() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let c = v.capabilities();
    assert!(c.can_count && c.can_insert && c.can_update && c.can_delete && c.can_import);
    assert!(c.can_order && c.can_search && c.can_filter_operators && c.can_subscribe);
    assert!(c.can_set_page_size && c.can_fetch_page && c.can_fetch_window);
    assert!(c.can_traverse_to_record && c.can_traverse_to_set);
    assert!(!c.can_invalidate && !c.can_fetch_next);
}

#[tokio::test]
async fn op_conditions_order_search_and_count() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(PRODUCT).unwrap();
    v.add_condition_op("price", FilterOp::Gte, int(4)).unwrap();
    v.add_order("price", SortDirection::Descending).unwrap();
    assert_eq!(ids(&v).await, ["p2", "p3"]);
    assert_eq!(v.get_count().await.unwrap(), 2);
    v.add_search("cof").unwrap();
    assert_eq!(ids(&v).await, ["p3"]);
}

#[tokio::test]
async fn add_order_replaces_previous_order() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(PRODUCT).unwrap();
    v.add_order("price", SortDirection::Descending).unwrap();
    v.add_order("name", SortDirection::Ascending).unwrap();
    assert_eq!(ids(&v).await, ["p2", "p3", "p1"]);
}

#[tokio::test]
async fn pages_and_windows() {
    let (_s, f) = setup();
    let mut v = f.from_yaml(PRODUCT).unwrap();
    v.add_order("price", SortDirection::Ascending).unwrap();
    v.set_page_size(2).unwrap();
    let p2: Vec<String> = v.fetch_page(2).await.unwrap().into_iter().map(|(id, _)| id).collect();
    assert_eq!(p2, ["p2"]);
    assert!(v.fetch_page(0).await.is_err());
    let w: Vec<String> = v.fetch_window(1, 1).await.unwrap().into_iter().map(|(id, _)| id).collect();
    assert_eq!(w, ["p3"]);
    assert_eq!(ids(&v).await.len(), 3, "a window must not narrow the vista itself");
}

#[tokio::test]
async fn writes_through_the_vista_reach_the_store() {
    let (s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let rec = [("name".to_string(), text("Scone")), ("price".to_string(), int(2))].into_iter().collect();
    let id = v.insert_return_id(&rec).await.unwrap();
    assert_eq!(id, "1");
    assert_eq!(s.table("product").len(), 4);
    let patch = [("price".to_string(), int(9))].into_iter().collect();
    v.patch_value(&id, &patch).await.unwrap();
    assert_eq!(s.table("product").get(&id).unwrap().get("price"), Some(&int(9)));
    v.delete(&id).await.unwrap();
    assert!(v.delete(&id).await.is_err());
}

#[tokio::test]
async fn clone_shell_isolates_query_state() {
    let (_s, f) = setup();
    let v = f.from_yaml(PRODUCT).unwrap();
    let mut narrowed = v.clone_vista().unwrap();
    narrowed.add_condition_eq("category", text("food")).unwrap();
    assert_eq!(ids(&narrowed).await, ["p2"]);
    assert_eq!(ids(&v).await.len(), 3);
}

#[tokio::test]
async fn has_one_reference_traverses_to_the_target_row() {
    let (s, f) = setup();
    let _products = f.from_yaml(PRODUCT).unwrap();
    let orders = f.from_yaml(ORDER).unwrap();
    s.table("order").upsert("o1", [("product".to_string(), text("p3")), ("qty".to_string(), int(2))].into_iter().collect());
    let row = s.table("order").get("o1").unwrap();
    let target = orders.get_ref("product", &row).unwrap();
    assert_eq!(ids(&target).await, ["p3"]);
    assert!(target.columns().contains_key("price"), "target uses the catalog's metadata");
}

#[tokio::test]
async fn seed_file_is_loaded_by_the_factory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cat.yaml");
    std::fs::write(&path, "- { id: c1, name: Cat }\n").unwrap();
    let store = MemoryStore::new();
    let f = MemoryVistaFactory::new(store.clone());
    let yaml = format!("name: cat\nid_column: id\ncolumns:\n  id: {{ type: string, flags: [id] }}\n  name: {{ type: string }}\nmemory:\n  seed: {}\n", path.display());
    let v = f.from_yaml(&yaml).unwrap();
    assert_eq!(ids(&v).await, ["c1"]);
}
```

These tests use `Vista` convenience methods: `list_values`, `get_count`, `add_condition_op`, `add_condition_eq`, `add_order`, `add_search`, `set_page_size`, `fetch_page`, `fetch_window`, `insert_return_id`, `patch_value`, `delete`, `clone_vista`, `get_ref`, `columns`, `capabilities`. Check each name in `vantage-vista/src/vista.rs` (and the dataset trait impls it provides) and use the real names. The YAML shape follows `VistaSpec` (read `vantage-vista/src/spec.rs`; columns may be a list, or `id_column` may be derived from flags). Adjust the test YAML to the real schema, keeping the same content, and note any change.

- [ ] **Step 2: Run the tests and check they fail**

Run: `cargo test -p vantage-memory --test vista 2>&1 | tail -15`
Expected: compile errors.

- [ ] **Step 3: Implement**, following the table above. `vista.rs` holds the struct and capabilities, `vista/shell.rs` the trait impl, `vista/refs.rs` the reference code and the catalog, and `vista/factory.rs` the factory. If `shell.rs` grows past about 200 LOC, move the write methods into `vista/writes.rs` as private helpers the impl calls.

- [ ] **Step 4: Run the tests and check they pass**

Run: `cargo test -p vantage-memory --test vista 2>&1 | tail -15`
Expected: 8 tests pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt -p vantage-memory
git add vantage-memory/src vantage-memory/tests
git commit -m "vantage-memory: Vista shell, references and factory"
```

---

### Task 8: Native `watch_vista`, and a live Dio

**Files:**
- Create: `vantage-memory/src/vista/watch.rs`
- Modify: `vantage-memory/src/vista/shell.rs` (`watch_vista` delegates to `watch::stream`)
- Test: `vantage-memory/tests/watch.rs` and `vantage-memory/tests/dio.rs`

**Interfaces:**
- Consumes: `MemoryTable::subscribe`, `MemoryChange`, `MemoryTableShell::{query, table}`, `eval::matches_all`.
- Produces: `watch::stream(table: MemoryTableHandle, query: Query) -> VistaChangeStream`.

Behaviour, per change and per subscriber, checking only the conditions and search of a snapshot of the shell's query taken when `watch_vista` is called:

| Change | old matched? | new matched? | Emits |
|---|---|---|---|
| `Inserted { row }` | – | yes | `Inserted { id, value: (*row).clone() }` |
| `Inserted` | – | no | nothing |
| `Updated { row, old }` | no | yes | `Inserted` |
| `Updated` | yes | yes | `Updated` |
| `Updated` | yes | no | `Deleted` |
| `Updated` | no | no | nothing |
| `Deleted { old }` | yes | – | `Deleted` |
| `Deleted` | no | – | nothing |
| channel `Lagged(_)` | | | `Invalidated`, then continue |
| channel `Closed` | | | end the stream |

A condition that errors while matching (a `Column` or unresolved `Deferred` should never reach here) ends the stream with that error. Build the stream with `async_stream::try_stream!`, following surreal's `watch_vista` in traits.md §7.

- [ ] **Step 1: Write the failing tests**

`vantage-memory/tests/watch.rs`:

```rust
use std::time::Duration;

use ciborium::Value as CborValue;
use futures_util::StreamExt;
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_vista::{VistaChange, VistaFactory};

const T: &str = "name: t\nid_column: id\ncolumns:\n  id: { type: string, flags: [id] }\n  status: { type: string }\n";

fn text(s: &str) -> CborValue {
    CborValue::Text(s.into())
}
fn status(s: &str) -> vantage_types::Record<CborValue> {
    [("status".to_string(), text(s))].into_iter().collect()
}

async fn next(s: &mut vantage_vista::VistaChangeStream) -> VistaChange {
    tokio::time::timeout(Duration::from_secs(2), s.next()).await.expect("change").expect("open").unwrap()
}

async fn nothing(s: &mut vantage_vista::VistaChangeStream) {
    assert!(tokio::time::timeout(Duration::from_millis(150), s.next()).await.is_err());
}

#[tokio::test]
async fn store_writes_reach_an_unfiltered_watch() {
    let store = MemoryStore::new();
    let v = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    let mut w = v.watch().await.unwrap();
    let t = store.table("t");
    t.upsert("a", status("Open"));
    assert!(matches!(next(&mut w).await, VistaChange::Inserted { id, .. } if id == "a"));
    t.patch("a", &status("Closed"));
    assert!(matches!(next(&mut w).await, VistaChange::Updated { id, .. } if id == "a"));
    t.delete("a");
    assert!(matches!(next(&mut w).await, VistaChange::Deleted { id } if id == "a"));
}

#[tokio::test]
async fn vista_writes_reach_the_watch() {
    let store = MemoryStore::new();
    let v = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    let mut w = v.watch().await.unwrap();
    v.insert_return_id(&status("Open")).await.unwrap();
    assert!(matches!(next(&mut w).await, VistaChange::Inserted { .. }));
}

#[tokio::test]
async fn row_entering_filter_is_inserted() {
    let store = MemoryStore::new();
    let mut v = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    v.add_condition_eq("status", text("Open")).unwrap();
    let mut w = v.watch().await.unwrap();
    let t = store.table("t");
    t.upsert("a", status("Closed"));
    nothing(&mut w).await;
    t.patch("a", &status("Open"));
    assert!(matches!(next(&mut w).await, VistaChange::Inserted { id, .. } if id == "a"));
}

#[tokio::test]
async fn row_leaving_filter_is_deleted() {
    let store = MemoryStore::new();
    let t = store.table("t");
    t.upsert("a", status("Open"));
    let mut v = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    v.add_condition_eq("status", text("Open")).unwrap();
    let mut w = v.watch().await.unwrap();
    t.patch("a", &status("Closed"));
    assert!(matches!(next(&mut w).await, VistaChange::Deleted { id } if id == "a"));
    t.delete("a");
    nothing(&mut w).await;
}

#[tokio::test]
async fn lag_becomes_invalidated() {
    let store = MemoryStore::new();
    let v = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    let mut w = v.watch().await.unwrap();
    let t = store.table("t");
    for i in 0..5000 {
        t.upsert(&i.to_string(), status("x"));
    }
    let mut saw_invalidated = false;
    while let Ok(Some(c)) = tokio::time::timeout(Duration::from_millis(300), w.next()).await {
        if matches!(c.unwrap(), VistaChange::Invalidated) {
            saw_invalidated = true;
            break;
        }
    }
    assert!(saw_invalidated);
}
```

`vantage-memory/tests/dio.rs`. Before writing it, read traits.md §10 and `vantage-diorama/tests/dio_watch.rs` for the exact `Lens`/`Dio` API:

```rust
use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use vantage_diorama::Lens;
use vantage_memory::{MemoryStore, MemoryVistaFactory};
use vantage_vista::VistaFactory;

const T: &str = "name: t\nid_column: id\ncolumns:\n  id: { type: string, flags: [id] }\n  n: { type: int }\n";

async fn wait_for(dio: &vantage_diorama::Dio, id: &str, expect: Option<i64>) {
    for _ in 0..40 {
        let got = dio.cache().get_value(&id.to_string()).await.unwrap();
        let n = got.and_then(|r| match r.get("n") {
            Some(CborValue::Integer(i)) => Some(i128::from(*i) as i64),
            _ => None,
        });
        if n == expect {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{id} never reached {expect:?}");
}

#[tokio::test]
async fn dio_over_memory_stays_live_through_watch_alone() {
    let store = MemoryStore::new();
    let t = store.table("t");
    t.upsert("a", [("n".to_string(), CborValue::Integer(1.into()))].into_iter().collect());
    let master = MemoryVistaFactory::new(store.clone()).from_yaml(T).unwrap();
    let lens = Arc::new(
        Lens::new()
            .cache_in_memory()
            .on_start(|dio| {
                let dio = dio.clone();
                async move {
                    let rows = dio.master().list_values().await?;
                    dio.cache().insert_values(rows).await?;
                    Ok(())
                }
            })
            .build()
            .expect("lens"),
    );
    let dio = lens.make_dio(master).await.unwrap();
    assert!(dio.master().can_watch());
    dio.watch().await.unwrap();

    t.patch("a", &[("n".to_string(), CborValue::Integer(2.into()))].into_iter().collect());
    wait_for(&dio, "a", Some(2)).await;
    t.upsert("b", [("n".to_string(), CborValue::Integer(7.into()))].into_iter().collect());
    wait_for(&dio, "b", Some(7)).await;
    t.delete("a");
    wait_for(&dio, "a", None).await;
}
```

If the real `Dio::watch` returns a stream instead of spawning a task (traits.md §10 says `derived.watch().await?` is "the whole go-live call"), follow `dio_watch.rs` exactly. The test's intent is fixed: no `handle_event`, and the cache follows store writes.

- [ ] **Step 2: Run the tests and check they fail**

Run: `cargo test -p vantage-memory --test watch 2>&1 | tail -15`
Expected: failures, because `watch_vista` still returns the trait's default error.

- [ ] **Step 3: Implement** `watch.rs`, following the table above, and override `watch_vista` in `shell.rs` to call `watch::stream(self.table.clone(), self.query.clone())`.

- [ ] **Step 4: Run the tests and check they pass**

Run: `cargo test -p vantage-memory --test watch 2>&1 | tail -15`, then `cargo test -p vantage-memory --test dio 2>&1 | tail -15`
Expected: 5 + 1 tests pass.

- [ ] **Step 5: Commit**

```bash
cargo fmt -p vantage-memory
git add vantage-memory/src vantage-memory/tests
git commit -m "vantage-memory: native watch_vista; a Dio stays live without forwarding"
```

---

### Task 9: Bench example, README, CHANGELOG and spec sync

**Files:**
- Create: `vantage-memory/examples/bench.rs`
- Create: `vantage-memory/README.md`
- Create: `vantage-memory/CHANGELOG.md`
- Modify: `vantage-memory/SPEC.md` (fold in the "Spec deltas" at the top of this plan)

- [ ] **Step 1: Write the bench example**

For each size N in 1_000, 10_000 and 100_000, on a fresh `MemoryStore`, with a table defined with `indexed: ["status"]`:
- **insert:** N rows `{ status: one of 5 values, n: i, name: "row i" }`
- **patch:** N patches of `n`
- **get:** N random `get`s
- **indexed query:** 1000 `Query`s of `status Eq <value>`
- **range query:** 100 `Query`s of `n Gt N/2`, with no index
- **ordered page:** 100 `Query`s ordered by `n` descending with a window of `(N/2, 50)`

Print one line per operation per size: `size  op  ops/s  µs/op`. Use `std::time::Instant` and nothing else, with no criterion. Run it once and check the numbers print:

Run: `cargo run -p vantage-memory --example bench --release 2>&1 | tail -20`
Expected: 18 lines of numbers. Nothing is asserted.

- [ ] **Step 2: Write the README.** Plain, direct engineer prose (no marketing tone):
  1. What it is: an in-memory vantage datasource with both layers and live changes. It is not a mock; mocks stay for tests.
  2. Quick use: the sync store (`MemoryStore::table`, `insert`/`upsert`/`patch`/`delete`/`query`); the typed layer (`MemoryDB` + `Table`, with an example condition, order and aggregate); a Vista via `MemoryVistaFactory::from_yaml`, with the YAML extras `memory: { indexed, seed }`; live changes via `watch()`, or a Dio.
  3. Semantics: comparisons (int/float), nulls, `Like`, search, ordering (nulls first ascending, stable ties), dotted paths, ids (counter/prefix/supplied), no-op writes send nothing, quiet mode.
  4. Performance notes and how to run the bench, with the last run's numbers pasted as a small table (debug and release both labelled).
  5. Limits: no persistence, no transactions, no `aggregate_vista`.

- [ ] **Step 3: Write the CHANGELOG**

```markdown
# Changelog

## 0.6.0 — 2026-09-29

- In-memory datasource: sync store with per-table locks, `Arc`-shared rows and hash indexes.
- Typed `TableSource` (`MemoryDB`) with every `FilterOp`, search, ordering, paging, count/sum/min/max and references.
- Vista `MemoryTableShell` + `MemoryVistaFactory` (`memory: { indexed, seed }`), with native `watch_vista`.
- Seed load/dump from JSON or YAML.
```

- [ ] **Step 4: Sync SPEC.md.** Apply each of the six spec deltas at the top of this plan to the matching SPEC.md section:
  - `MemoryChange` variants
  - the single lock per table
  - `MemoryCondition::Column` and `.ascending()/.descending()`
  - `SortDirection` and null placement
  - the shell sitting on the store
  - the reference catalog

  Also record any names that changed during implementation, listed in the task reports.

- [ ] **Step 5: Final checks.** Run each as its own command:
  - `cargo test -p vantage-memory 2>&1 | tail -5`: all pass
  - `cargo clippy -p vantage-memory --all-targets 2>&1 | grep -c "^warning"`: 0
  - `cargo build --workspace 2>&1 | tail -3`: the workspace still builds with the new member

- [ ] **Step 6: Commit**

```bash
cargo fmt -p vantage-memory
git add vantage-memory
git commit -m "vantage-memory: bench example, README, changelog, spec synced"
```
