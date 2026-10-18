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
The block reads and writes a launch database, so it is compiled but not run:

```rust,no_run
# use vantage_dataset::prelude::ActiveEntitySet;
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# pub struct Launch {
#     pub name: String,
# }
#
# impl Launch {
#     pub fn table(db: SqliteDB) -> Table<SqliteDB, Launch> {
#         Table::new("launch", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_many("launch_crew", "launch_id", LaunchCrew::table)
#     }
# }
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# pub struct LaunchCrew {
#     pub launch_id: Option<String>,
#     pub astronaut_id: Option<String>,
#     pub role: Option<String>,
# }
#
# impl LaunchCrew {
#     pub fn table(db: SqliteDB) -> Table<SqliteDB, LaunchCrew> {
#         Table::new("launch_crew", db)
#             .with_id_column("id")
#             .with_column_of::<String>("launch_id")
#             .with_column_of::<String>("astronaut_id")
#             .with_column_of::<String>("role")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:launches.db").await.context("connect")?;
# let launches = Launch::table(db);
# let (id, a) = ("apollo-11", "armstrong".to_string());
use vantage_table::prelude::GetRefExt;

let launch = launches.get_entity(id).await?.expect("launch");

// A child set scoped to this launch — and carrying its foreign key (see invariants below).
let crew = launch.get_ref::<LaunchCrew>("launch_crew")?;
crew.insert_return_id(&LaunchCrew { astronaut_id: Some(a), role: Some("Pilot".into()), ..Default::default() }).await?;
# Ok(())
# }
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
- a Vista's `add_condition` with `FilterOp::Eq`, which `Vista::get_ref` uses for each join key.
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

```rust,no_run
# use vantage_dataset::prelude::ActiveEntitySet;
# use vantage_sql::prelude::*;
# use vantage_table::prelude::GetRefExt;
# use vantage_types::prelude::*;
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# pub struct Launch {
#     pub name: String,
# }
#
# impl Launch {
#     pub fn table(db: SqliteDB) -> Table<SqliteDB, Launch> {
#         Table::new("launch", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_many("launch_crew", "launch_id", LaunchCrew::table)
#     }
# }
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# pub struct LaunchCrew {
#     pub launch_id: Option<String>,
#     pub astronaut_id: Option<String>,
#     pub role: Option<String>,
# }
#
# impl LaunchCrew {
#     pub fn table(db: SqliteDB) -> Table<SqliteDB, LaunchCrew> {
#         Table::new("launch_crew", db)
#             .with_id_column("id")
#             .with_column_of::<String>("launch_id")
#             .with_column_of::<String>("astronaut_id")
#             .with_column_of::<String>("role")
#     }
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite:launches.db").await.context("connect")?;
# let launches = Launch::table(db);
# let launch = launches.get_entity("apollo-11").await?.expect("launch");
# let a = "armstrong".to_string();
let crew = launch.get_ref::<LaunchCrew>("launch_crew")?;

// launch_id absent → filled from the set:
crew.insert_return_id(&LaunchCrew { astronaut_id: Some(a), role: Some("Pilot".into()), ..Default::default() }).await?;

// launch_id set to a *different* launch → Err:
assert!(crew.insert_return_id(&LaunchCrew { launch_id: Some("other".into()), ..Default::default() }).await.is_err());
# Ok(())
# }
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
# use vantage_sql::prelude::*;
# use vantage_table::table::{BeforeCx, BoxFuture};
# use vantage_types::prelude::*;
#
# #[entity(SqliteType)]
# #[derive(Debug, Clone, Default)]
# pub struct Launch {
#     pub name: String,
# }
#
# impl Launch {
#     pub fn table(db: SqliteDB) -> Table<SqliteDB, Launch> {
#         Table::new("launch", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<String>("created")
#     }
# }
#
# fn stamp_created(cx: BeforeCx<'_, SqliteDB>) -> BoxFuture<'_, VantageResult<()>> {
#     Box::pin(async move {
#         cx.record.insert("created".into(), AnySqliteType::new("2026-10-14".to_string()));
#         Ok(())
#     })
# }
#
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite::memory:").await.context("connect")?;
use vantage_table::prelude::{Hook, Phase};
use vantage_table::table::before_hook;

let launches = Launch::table(db).with_hook(Hook::BeforeInsert(Phase::Populate, before_hook(stamp_created)));
# let _ = launches;
# Ok(())
# }
```

### The `Hook` variants

One enum carries a placement-specific closure. Each hook receives one context struct holding what's
available at its stage, so new fields can be added without breaking existing hooks:

| variant | when | receives | may |
| --- | --- | --- | --- |
| `BeforeInsert(Phase, _)` | before an insert | `BeforeCx { record: &mut Record, context, table }` | mutate / `Err` to cancel |
| `BeforeUpdate(Phase, _)` | before replace/patch | `BeforeCx` | mutate / `Err` to cancel |
| `BeforeSave(Phase, _)` | before insert **and** update | `BeforeCx` | mutate / `Err` to cancel |
| `BeforeDelete(_)` | before a delete | `DeleteCx { id, record (former), context, table }` | `Err` to veto, or `HookReturn::Handled` to take over |
| `AfterInsert` / `AfterUpdate` / `AfterSave` | after the write commits | `AfterCx { id, record, before, context, table }` | side-effects |
| `AfterDelete(_)` | after a delete | `AfterCx` (`record` is the former row) | side-effects |
| `BeforeDeleteAll(_)` / `AfterDeleteAll(_)` | around `delete_all` | `DeleteAllCx { context, table }` | `Err` to cancel (before) / side-effects |
| `BeforeSelect(_)` | before a read: list, get, stream, count, sum/max/min | `SelectCx { context, table }` | `Err` to refuse the read |

The `table` handed to a hook is entity-erased, so it can traverse relations (`get_ref`) and reach
the datasource — enough for cross-row validation and after-effects. `context` is the
[`WriteContext`](vantage_table::write_context::WriteContext) in scope for the write, if any.

### Ordering and control flow

- **Before-write hooks run ahead of invariant enforcement**, ordered by `Phase`:
  `Normalize` → `Populate` (the default) → `Validate`, then registration order within a phase. So
  normalize inputs, then derive/stamp fields, then validate the final record.
- **Returning `Err` from any before-hook cancels the operation** before anything is written.
- **`BeforeDelete` returning `HookReturn::Handled`** skips the real delete and reports success —
  this is how soft-delete works (patch a `deleted` marker, return `Handled`). A delete with hooks
  loads the row once so before/after hooks see its contents. A delete with hooks on a row outside
  the set runs no hooks and deletes nothing.
- **An update with after-update hooks reads the row first**, through the set, so `cx.before`
  holds the row as it was; without such hooks there is no extra read.
- **`delete_all` runs `BeforeDeleteAll` / `AfterDeleteAll` once for the whole set**, never the
  per-row delete hooks. The hook gets the table, whose `conditions()` describe the set; an error
  from the before hook cancels it. A soft-delete `BeforeDelete` doesn't cover it: `delete_all`
  still removes the rows, so a soft-deleting table needs a `BeforeDeleteAll` that refuses it.
- After-hooks run for side-effects only. Vantage favours idempotence over transactions: an
  after-hook failure surfaces an error but does not roll back the committed write — design
  after-effects to be safe to retry.

### Read hooks

`BeforeSelect` is the one hook on the read side. It runs before every read that returns rows or
facts about rows, so a rule like "only signed-in callers read invoices" lives on the table rather
than in every handler that lists it. A stream yields the hook's error as its first item. The hook
reads `cx.context` (the same [`WriteContext`](vantage_table::write_context::WriteContext) writes
see, so one scope covers a request's reads and writes) and `cx.table.conditions()`, which describe
the set about to be read:

```rust
use vantage_sql::sqlite::SqliteDB;
use vantage_table::table::{SelectCx, select_hook};

let signed_in_only = select_hook(|cx: SelectCx<'_, SqliteDB>| {
    Box::pin(async move {
        match cx.context.and_then(|c| c.principal.as_ref()) {
            Some(_) => Ok(()),
            None => Err(vantage_core::error!("sign in to read this table")),
        }
    })
});
# let _ = signed_in_only;
```

It does **not** run when the table is only embedded in another query: `related_in_condition`,
`column_table_values_expr`, `get_*_query` and `get_expr_*` build expressions, not reads of this
table. A rule that must hold inside a subquery belongs in the table's conditions, added when the
table is built (see the build injector in
[Config-Driven Vistas](./config-driven-vistas.md#the-build-injector)).

A vista shell that reads the data source directly, without going through `Table` (a SQL count, a
REST window, a live watch), calls
[`Table::check_read`](vantage_table::table::Table::check_read) first, so the hook guards those
reads too.

Reads a write makes run the hook as well, so a refusing `BeforeSelect` also refuses:

- an update once the table has an after-update hook, since the update reads `cx.before` first;
- a delete once the table has delete hooks, since the delete reads the row for them;
- a write whose backend reads the row back after it (an insert, replace or patch that returns the
  stored row).

So registering an audit hook can change whether a caller allowed to write but not to read can
update. That is deliberate: a refused read must not leak the former row into a hook.

Reads served by a Dio's cache are the exception; see the Dio note below.

### Writing a hook

Hooks are boxed async closures over their context struct. `before_hook`, `before_delete_hook` and
`after_hook` wrap a closure or a free `fn` into the hook type; their bounds guide closure
inference:

```rust
use vantage_types::Record;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::prelude::HookReturn;
use vantage_table::table::{BeforeCx, DeleteCx, before_delete_hook, before_hook};
# use vantage_dataset::WritableValueSet;
#
# fn now_iso8601() -> String {
#     "2026-10-14T09:00:00Z".to_string()
# }

// before-insert: stamp an audit field
let stamp_created = before_hook(|cx: BeforeCx<'_, SqliteDB>| {
    Box::pin(async move {
        cx.record.insert("created".into(), AnySqliteType::new(now_iso8601()));
        Ok(())
    })
});

// before-delete: soft-delete instead of removing the row
let soft_delete = before_delete_hook(|cx: DeleteCx<'_, SqliteDB>| {
    Box::pin(async move {
        let mut patch = Record::new();
        patch.insert("deleted".into(), AnySqliteType::new(now_iso8601()));
        cx.table.patch_value(cx.id.clone(), &patch).await?;
        Ok(HookReturn::Handled)
    })
});
# let _ = (stamp_created, soft_delete);
```

Who made a write comes from the
[`WriteContext`](vantage_table::write_context::WriteContext): a host wraps its work in
`WriteContext::new().with_principal(p).with_group(g).scope(fut)`, and every write awaited inside
it hands the context to its hooks as `cx.context`. Outside a scope, `cx.context` is `None`. The
context and the hook context structs are `#[non_exhaustive]`: build a `WriteContext` with `new()`
and the `with_*` builders, and read a `cx` by field or destructure it with `..`, so a field added
later breaks neither.

Writes through a Dio need one more step, because a Dio runs them on its own worker task where the
caller's scope doesn't reach. [`Dio::flash`](vantage_diorama::Dio::flash) records the context in
scope where it is called on the [`ChangeFlash`](vantage_diorama::ChangeFlash), and the worker runs
the write inside it, so the master table's hooks (and a Lens writer) see the same context.
`ChangeFlash::with_context` sets one explicitly. A `clear` the Dio runs as one delete per row
passes its context to each of those deletes.

Reads are another matter. A Dio's `list`, `count` and window serve its cache, which the loader
fills from its own task with no `WriteContext` in scope, so reads through a Dio don't run the
master table's `BeforeSelect` per caller. A per-caller read rule belongs in the conditions of the
table the Dio is built over, or in a Dio per principal.
