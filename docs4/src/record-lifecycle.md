# Records: Traversal, Invariants & Hooks

Once a backend implements `TableSource` (see [Adding a New Persistence](./new-persistence.md)),
`vantage-table` gives every `Table<Db, Entity>` a uniform write pipeline. On each write the record
flows through three stages before it reaches the datasource:

```text
record  →  lifecycle hooks (before)  →  set invariants  →  datasource write  →  hooks (after)
```

This page covers the consumer-facing features built on that pipeline: traversing a relation from a
loaded record, the foreign-key **invariants** that traversal sets up, and the **lifecycle hooks**
you can attach for audit, validation, soft-delete, and after-effects. They are backend-agnostic —
everything here works the same on SQLite, Postgres, MySQL, Mongo, etc.

## Traversing from a loaded record: `get_ref`

`Table::get_ref_as` / `get_ref_from_row` traverse a relation from a *table* (both covered in
[Relations & Traversal](relations/traversal.md)). The `GetRefExt` trait adds the record-level
equivalent: traverse straight from a loaded `ActiveEntity` (typed) or `ActiveRecord` (untyped).

```rust
use vantage_table::prelude::GetRefExt;

let launch = launches.get_entity(id).await?.expect("launch");

// A child set scoped to this launch — and carrying its foreign key (see invariants below).
let crew = launch.get_ref::<LaunchCrew>("launch_crew")?;
crew.insert_return_id(&LaunchCrew { astronaut_id: Some(a), role: Some("Pilot".into()), ..Default::default() }).await?;
```

`get_ref::<E2>(relation)` returns a `Table<T, E2>` scoped to the parent. For a typed `ActiveEntity`
the entity's id is injected into the row before traversal (so has-many relations resolve); an
untyped `ActiveRecord` already holds the raw row and forwards directly. The same method exists on
both handles — `GetRefExt` carries blanket implementations for each, generic over any
`TableSource` (the handles themselves live in `vantage-dataset` and know nothing about `Table`,
which is why this arrives as an extension trait).

## Safe writes

Every write is retry-safe, and a write through a narrowed table only touches rows in it. An
unnarrowed table is the only way to write anywhere.

The set is the table's conditions, equality and operator alike. For a Vista or a Rhai handle it
is every narrowing step that decides membership.

| operation | row in the set | row outside the set | no such row |
| --- | --- | --- | --- |
| `delete(id)` | deleted; `Ok(())` | left alone; `Ok(())` | `Ok(())` |
| `insert(id, rec)` | nothing written; returns the stored row | `Conflict`, where the backend can tell | inserted; the record must fit the set |
| `patch(id, partial)` | applied; `Conflict` if the row would leave the set | `NotFound` | `NotFound` |
| `replace(id, rec)` | applied; `Conflict` if the row would leave the set | `Conflict`, where the backend can tell | created; the record must fit the set |
| import | `skipped` | `rejected` | inserted; `rejected` if it doesn't fit the set |
| `delete_all()` | deletes exactly the set | — | — |

A write that isn't allowed fails with `ErrorKind::Unsupported`: writes are denied, the capability
is off, or the backend can't apply one of the set's conditions. In Rhai, `delete` returns `false`
instead and the other verbs throw.

The data source enforces the set: SQL adds the conditions to the statement and checks a written
row with a one-row probe; SurrealDB and MongoDB do the same in their own query language; memory,
redb, DynamoDB and Dio evaluate the conditions on the row. A source that can't apply or evaluate a
condition refuses the write.

Ids: a record without an id gets a client-made UUIDv7 before the first attempt (Rhai `insert`, a
new record, Servo `IdStrategy::Uuid`,
[`IdGenerator::UuidV7`](vantage_table::table::IdGenerator)), so a retry lands on the idempotent
insert. A table whose ids the server makes declares it
([`Table::with_auto_id`](vantage_table::table::Table::with_auto_id), YAML `flags: [id, auto]`) and
inserts through `insert_return_id`, which is not retry-safe.

```admonish warning title="Eventually-consistent backends"
A backend that can't see a row before it writes can't tell an insert of an id held outside the set
from a new row. Such a backend documents what it does instead; vantage never overwrites silently.
DynamoDB inserts with a conditional put, so it does detect the clash.
```

There are no transactions. `replace` and `patch` check the row and then write it, so a concurrent
writer can change the row between the two. MySQL can't run an `UPDATE` or `DELETE` whose
condition subqueries the table being written (error 1093), so a set defined that way can't be
written through on MySQL.

## Set invariants

A table narrowed by a literal `column = value` is a **set**, and every row written into it must
*conform* to that definition. Vantage records the pair as an **invariant** and enforces it on
write. The most common source is relationship traversal: `launch.get_ref::<LaunchCrew>("launch_crew")`
narrows the child set by `launch_id = <this launch>`, so an inserted crew row's `launch_id` is
filled automatically.

Invariants are registered automatically wherever the scope is a plain `column = value`:

- `Table::with_id(id)` — narrows by the id column.
- has-many / has-one traversal (`Reference::resolve_from_row`) — narrows by the foreign key.
- **every literal `column = value` passed to `add_condition` / `with_condition`**.
- a Vista's `add_condition_eq`.
- Rhai `where(col, value)`.

Expression scopes never register an invariant. You can also set one explicitly with
`Table::with_invariant(column, value)` / `add_invariant`.

On insert and replace, each invariant column is resolved by a four-way rule:

| record's value for the column | result |
| --- | --- |
| absent | set to the invariant value |
| present but null | set to the invariant value |
| present and equal | kept |
| present and **conflicting** | the write is rejected with an error |

So a child row inserted through a relation needs no foreign key (it's filled), may state the
matching one (kept), but cannot smuggle in a *different* one (error — it doesn't belong to this set).

A patch never fills an invariant column. A present value must match it; a different or null value
is rejected (`Conflict`), because it would move the row out of the set. Moving a row between sets
goes through a handle whose set holds both.

```rust
let crew = launch.get_ref::<LaunchCrew>("launch_crew")?;

// launch_id absent → filled from the set:
crew.insert_return_id(&LaunchCrew { astronaut_id: Some(a), role: Some("Pilot".into()), ..Default::default() }).await?;

// launch_id set to a *different* launch → Err:
assert!(crew.insert_return_id(&LaunchCrew { launch_id: Some("other".into()), ..Default::default() }).await.is_err());
```

```admonish info title="Backends and InvariantValue"
Enforcement needs two operations on a backend's value type — a null check and an equality check —
provided by the `InvariantValue` trait (`vantage-types`). The `vantage_type_system!` macro emits it
for every generated `Any*Type`; pass a `null_when:` pattern (e.g. `null_when: ciborium::Value::Null`
for the SQL backends) so genuine nulls are recognised. Non-nullable value types (e.g. CSV's
`String`) simply never match the null branch.
```

## Lifecycle hooks

Attach async callbacks around writes with `Table::with_hook(Hook::…)`. Hooks are how audit stamps,
normalization, validation, soft-delete, and after-effects are expressed — generically, on any
backend.

```rust
use std::sync::Arc;
use vantage_table::prelude::{Hook, Phase};

let launches = Launch::table(db).with_hook(Hook::BeforeInsert(Phase::Populate, Arc::new(stamp_created)));
```

### The `Hook` variants

One enum carries a placement-specific closure, so each hook receives exactly what's available at
its stage. Before-write hooks get the record (mutable) and the entity-erased table; the delete hook
gets the id and the row's former contents:

| variant | when | receives | may |
| --- | --- | --- | --- |
| `BeforeInsert(Phase, _)` | before an insert | `&mut Record`, `&Table` | mutate / `Err` to cancel |
| `BeforeUpdate(Phase, _)` | before replace/patch | `&mut Record`, `&Table` | mutate / `Err` to cancel |
| `BeforeSave(Phase, _)` | before insert **and** update | `&mut Record`, `&Table` | mutate / `Err` to cancel |
| `BeforeDelete(_)` | before a delete | `&Id`, `&Record` (former), `&Table` | `Err` to veto, or `HookReturn::Handled` to take over |
| `AfterInsert` / `AfterUpdate` / `AfterSave` | after the write commits | `&Id`, `&Record`, `&Table` | side-effects |
| `AfterDelete(_)` | after a delete | `&Id`, `&Record` (former), `&Table` | side-effects |

The `&Table` handed to a hook is entity-erased, so it can traverse relations (`get_ref`) and reach
the datasource — enough for cross-row validation and after-effects.

### Ordering and control flow

- **Before-write hooks run ahead of invariant enforcement**, ordered by `Phase`:
  `Normalize` → `Populate` (the default) → `Validate`, then registration order within a phase. So
  normalize inputs, then derive/stamp fields, then validate the final record.
- **Returning `Err` from any before-hook cancels the operation** before anything is written.
- **`BeforeDelete` returning `HookReturn::Handled`** skips the real delete and reports success —
  this is how soft-delete works (patch a `deleted` marker, return `Handled`). A delete with hooks
  loads the row once so before/after hooks see its contents; `delete_all` fires no hooks. A
  delete with hooks on a row outside the set runs no hooks and deletes nothing.
- After-hooks run for side-effects only. Vantage favours idempotence over transactions: an
  after-hook failure surfaces an error but does not roll back the committed write — design
  after-effects to be safe to retry.

### Writing a hook

Hooks are boxed async closures. The reliable construction is a free `fn` returning a boxed future
whose lifetime ties to the arguments, then `Arc::new(it)`:

```rust
use std::future::Future;
use std::pin::Pin;
use vantage_core::Result;
use vantage_types::Record;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;

// before-insert: stamp an audit field
fn stamp_created<'a>(
    rec: &'a mut Record<AnySqliteType>,
    _t: &'a Table<SqliteDB, vantage_types::EmptyEntity>,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        rec.insert("created".into(), AnySqliteType::new(now_iso8601()));
        Ok(())
    })
}

// before-delete: soft-delete instead of removing the row
fn soft_delete<'a>(
    id: &'a String,
    _former: &'a Record<AnySqliteType>,
    table: &'a Table<SqliteDB, vantage_types::EmptyEntity>,
) -> Pin<Box<dyn Future<Output = Result<vantage_table::prelude::HookReturn>> + Send + 'a>> {
    Box::pin(async move {
        let mut patch = Record::new();
        patch.insert("deleted".into(), AnySqliteType::new(now_iso8601()));
        table.patch_value(id.clone(), &patch).await?;
        Ok(vantage_table::prelude::HookReturn::Handled)
    })
}
```

To pull in outside context (e.g. an `updated_by` actor), have the hook capture an `Arc` or read a
task-local — the closure runs in async context. A capturing closure works too, but needs its boxed
return type annotated explicitly so it coerces to the hook type.
