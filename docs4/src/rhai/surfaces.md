# Surfaces

The data vocabulary is the same everywhere. What changes from one place to the next is the host:
which tables `table(name)` reaches, which terminal verbs exist, and what the script is given in
scope. This chapter goes through each surface with a tested example.

<!-- toc -->

---

## Overview

Rows marked "(app)" are hosts an application builds for itself; the values shown are Vantage UI's.
The rest come with vantage.

| Surface | `table(name)` resolves | Terminals | Limits | In scope |
|---|---|---|---|---|
| `run_script` (agent tools) | the caller's resolver | ReadWrite, `list()` capped at 1 to 50 rows | Background | |
| `preview_script` | the caller's resolver | Describe | Background | |
| YAML `modify:` | the backend's spec resolver | Describe | Background | `self` |
| reference build script | the backend's spec resolver | Describe | Background | `row` |
| `lazy:` computed column | none | none: no data vocabulary | Background | `row` |
| form `options:` (app) | the app's tables | ReadWrite, writes denied, capped | | form values |
| action predicates, `when:` (app) | none | ReadWrite, writes denied | Ui | `row` |
| action bodies (app) | the app's tables, writes through Dio | ReadWrite | Ui | `row`, `actions` |
| form `on_submit`, wizard forms (app) | the app's tables, writes through Dio | ReadWrite | Background | `form` (a Servo), `wizard` |
| wizard `worker:` (app) | the app's tables | ReadWrite, 50 rows | Background | `wizard`, `state` |
| faker sims | the sim's memory store | ReadWrite | the sim's own budget | `args` |

## Agent scripts: `run_script`

[`run_script`](vantage_vista::rhai::run_script) evaluates a script with every read and write verb and
returns its last value as JSON. It runs under `spawn_blocking` for you:

<!-- tested: rhai_guide::surfaces::agent_script -->
```rhai
let due = table("order").where("status", "due");
#{ count: due.count(), rows: due.sort("total", "desc").list() }
```

```rust
# use std::sync::Arc;
# use ciborium::Value as CborValue;
# use vantage_memory::{MemoryCatalog, MemoryStore};
# use vantage_vista::rhai::{Writes, run_script};
# use vantage_vista::{Column, ColumnFlag, VistaMetadata, VistaResolver};
# const AGENT_SCRIPT: &str = r#"
# let due = table("order").where("status", "due");
# #{ count: due.count(), rows: due.sort("total", "desc").list() }
# "#;
# fn resolver(store: &MemoryStore) -> Arc<dyn VistaResolver> {
#     let catalog = MemoryCatalog::new(store.clone());
#     catalog.register(
#         "order",
#         VistaMetadata::new()
#             .with_column(Column::new("id", "String").with_flag(ColumnFlag::Id))
#             .with_column(Column::new("total", "int").with_flag(ColumnFlag::Orderable))
#             .with_column(Column::new("status", "String"))
#             .with_id_column("id"),
#     );
#     Arc::new(catalog)
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let store = MemoryStore::new();
# for (id, total, status) in [("o1", 120, "paid"), ("o2", 40, "due"), ("o4", 15, "due")] {
#     let row = [
#         ("total".to_string(), CborValue::Integer(total.into())),
#         ("status".to_string(), CborValue::Text(status.into())),
#     ]
#     .into_iter()
#     .collect();
#     store.table("order").insert_as(id, row)?;
# }
let out = run_script(
    AGENT_SCRIPT.to_string(),
    resolver(&store),
    1,
    Writes::Denied("writing data is turned off".into()),
)
.await
.unwrap();
# assert_eq!(out["count"], 2);
# assert_eq!(out["rows"].as_array().unwrap().len(), 1);
# Ok(())
# }
```

The limit is clamped to `MIN_LIMIT..=MAX_LIMIT` (1 to 50; `DEFAULT_LIMIT` is 5) and caps every
`list()`, so `rows` above holds one row while `count` is 2. This is an inspection surface, not a
bulk reader. Compile, runtime and backend errors all come back as the error string.

Vantage UI's MCP `run_data_script` tool is this function. Its writes stay `Denied` unless the user
turns on "Allow MCP agents to write data", a setting separate from the one that lets agents read.

## Query preview: `preview_script`

[`preview_script`](vantage_vista::rhai::preview_script) runs a script on a `Describe` host, so it
can build a handle but never read rows. The script must end on the handle. The resolved Vista
renders its query through [`Vista::preview`](vantage_vista::Vista::preview):

<!-- tested: rhai_guide::surfaces::preview -->
```rhai
table("order").where("status", "paid").sort("total", "desc")
```

The result is JSON with the Vista's `driver` and the `query` it would run, plus a `note` naming
anything the query leaves out. Here the memory store reports `driver: "memory"` and `query: "scan
order WHERE status = \"paid\" ORDER BY total DESC"`. One exception to "never reads": a `ref` step
still reads the rows it starts
from, because the target's condition depends on them. When it finds none, the preview reports
`"query": null` with a note, because nothing would be queried.

## `modify:` scripts

A YAML spec's `modify:` script runs on the finished Vista, exposed as `self`. A `Describe` host
evaluates it with [`eval_modify_script`](vantage_vista::rhai::eval_modify_script):

<!-- tested: rhai_guide::surfaces::modify -->
```rhai
self.where("vip", true).sort("name", "desc")
```

The script's value is its last statement, and it must be a table handle: that handle is resolved
and becomes the table. A script that ends on anything else is an error, "The script must end on a
table handle". `modify:` narrows a real table, so the Vista stays writable. Reference build
scripts finish the same way.

### Backend extensions

A backend can add its own verbs to handles through
[`TableShell::register_rhai_extensions`](vantage_vista::TableShell::register_rhai_extensions),
which registers its functions and constants in one call. SurrealDB adds its whole expression vocabulary,
plus `with_condition(expr)`, which narrows by a native SurrealQL expression that `where` can't
spell:

<!-- tested: vantage-surrealdb tests/6_vista/query_sourced.rs modify_with_condition_narrows_a_writable_table -->
```rhai
self.with_condition(surreal::ident("is_paying_client") == true)
   .sort("name", "asc")
```

and `surreal::me`, the current-record anchor for graph paths. MySQL adds the SQL expression
vocabulary and the same `with_condition(expr)`; MongoDB's `with_condition` takes a filter written
as a Rhai map, `self.with_condition(#{price: #{"$gt": 100}})`, with `mongo::oid`, `mongo::date`
and `mongo::doc` for the BSON values a map can't spell.

A backend's free functions and constants live in its own namespace: `surreal::` for SurrealDB,
`sql::` for every SQL database, `mongo::` for MongoDB. So `table(name)` is always the data verb,
and a backend function never shadows one. The verbs on handles, `with_condition` included, and the
methods and operators on a backend's own types (`.from`, `.alias`, `["col"]`, `==`) take no
prefix.

`with_condition` narrows a Vista in hand, not a list of steps, so it works on `self` only. On a
handle from `table(name)`, or after a `ref` step, it throws an error naming the verb. A backend
implements it with
[`Handle::with_native_condition`](vantage_vista::rhai::Handle::with_native_condition), which hands
a [`NativeCondition`](vantage_vista::NativeCondition) to the shell's `add_native_condition`. This
stand-in passes an equality as the native condition:

<!-- tested: rhai_guide::surfaces::extension_chains -->
```rust
# use vantage_rhai::rhai::{Dynamic, EvalAltResult};
# use vantage_vista::rhai::{Handle, dynamic_to_cbor};
# use vantage_vista::{Condition, FilterOp, NativeCondition};
fn with_condition(h: &mut Handle, col: &str, value: Dynamic) -> Result<Handle, Box<EvalAltResult>> {
    let key = format!("{col} = {value}");
    let condition = Condition::new(col, FilterOp::Eq, dynamic_to_cbor(value)?);
    h.with_native_condition("with_condition", NativeCondition::new(condition, key))
        .map_err(|e| e.to_string().into())
}
```

`with_native_condition` narrows a copy of the Vista and returns a new handle over it, so like
every other verb it leaves the handle it was called on alone:

<!-- tested: rhai_guide::surfaces::extension_chains -->
```rhai
let all = self;
let vip = all.with_condition("vip", true);
all
```

still ends on every client. Verbs on `self` return new handles, so narrowing is a chain, whichever
verbs it uses:

<!-- tested: rhai_guide::surfaces::extension_chains -->
```rhai
self.with_condition("vip", true).with_condition("name", "Cy")
```

leaves only Cy, and

<!-- tested: rhai_guide::surfaces::extension_chains -->
```rhai
self.with_condition("vip", true).sort("name", "desc")
```

leaves Cy and Ada, in that order. Chain the calls, or assign a step with `let` and continue from
the variable. A statement that ends on a narrowing verb (`where`, `sort`, `search`, `limit`, `ref`,
`with_condition`) outside the script's last statement drops its handle, whatever the chain starts
from, and is refused when the script compiles, with "this statement's table handle is discarded;
chain the calls or assign it":

<!-- tested: rhai_guide::surfaces::extension_chains -->
```rhai
self.with_condition("vip", true);    // throws: this handle would be dropped
self.sort("name", "desc");
```

<!-- tested: rhai_guide::surfaces::extension_chains -->
```rhai
let all = self;
all.with_condition("vip", true);    // throws: this handle would be dropped
all
```

A last statement that isn't a handle is refused too. Watch an `if` with no `else`: when its
condition is false its value is `()`, so end it with `else { self }`:

<!-- tested: rhai_guide::surfaces::extension_chains -->
```rhai
let vip_only = false;
if vip_only {
    self.with_condition("vip", true)
}   // throws: a false `if` with no `else { self }` is ()
```

Backends without a scripting vocabulary (CSV, REST, SQLite and PostgreSQL Vistas) keep the
default no-op and get the data vocabulary alone.

## Reference build scripts

A reference whose join isn't a plain foreign key can carry a `rhai:` build script. It runs at
traversal time with the parent row as `row`, and must end on a handle:

<!-- tested: rhai_guide::surfaces::ref_build -->
```rhai
table("order").where("client", row.id).where("status", "due")
```

In YAML:

```yaml
references:
  due_orders:
    table: order
    kind: has_many
    foreign_key: client
    surreal:
      rhai: |
        table("order").where("client", row.id).where("status", "due")
```

[`eval_ref_script`](vantage_vista::rhai::eval_ref_script) evaluates it.

## Computed columns

A `lazy:` column script runs on the shared background host, with no data vocabulary at all: no
`table(name)`, no handles, only `row`. It computes one value from one row, every time the Vista
reads that row. [Computed columns](./computed.md) covers it.

## Read-only hosts

A read-only host is a `Terminals::ReadWrite` host with `writes: Writes::Denied(message)`. It reads
freely, and every write throws the message. Records still load, so a script can read
`row.status`, but flashing throws:

<!-- tested: rhai_guide::surfaces::read_host -->
```rhai
let o = table("order").record("o1");
o.status = "void";
o.flash()   // throws: this host only reads
```

Table write verbs such as `delete` throw the same message. Vantage UI uses read-only hosts for
form `options:` scripts, and for action predicates, which get `row` but no `table(name)`.

## Read-write hosts in an application

An application decides what its action scripts see; Vantage UI, for example, gives action bodies
and form `on_submit` scripts `table(name)` over the app's tables and, for row actions, `row` as a
[record](./records.md):

```rhai
let r = actions.cancel_order(row);
row.status = "cancelled";
row.cancellation_reason = r.reason;
row.flash();
```

The resolver behind `table(name)` sends writes through each table's Dio, so open pages see them
at once, and applies the app's write rules (read-only tables, reference coercion, generated ids).
Form and wizard scripts also see Servo drafts, which have their own vocabulary
([Layers](./layers.md)). The action and form vocabularies themselves (`actions.*`, `notify`,
`form`) are documented in Vantage UI's skills.

## Faker sims

Sims are covered in [Faker Sims](./faker.md): they use this vocabulary over a memory store, with
`table()` naming the sim's own table, plus time and random verbs.
