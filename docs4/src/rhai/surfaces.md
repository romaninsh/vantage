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
| augmentation source | the catalog | Describe | Background | `self`, `row` |
| `lazy:` computed column | none | none: no data vocabulary | Background | `row` |
| form `options:` (app) | the app's tables | Read, capped | | form values |
| action predicates, `when:` (app) | none | Read | Ui | `row` |
| action bodies (app) | the app's tables, writes through Dio | ReadWrite | Ui | `row`, `actions` |
| form `on_submit`, wizard forms (app) | the app's tables, writes through Dio | ReadWrite | Background | `form` (a Servo), `wizard` |
| wizard `worker:` (app) | the app's tables | ReadWrite, 50 rows | Background | `wizard`, `state` |
| faker sims | the sim's memory store | ReadWrite | the sim's own budget | `args` |

## Agent scripts: `run_script`

[`run_script`](vantage_vista::run_script) evaluates a script with every read and write verb and
returns its last value as JSON. It runs under `spawn_blocking` for you:

<!-- tested: rhai_guide::surfaces::agent_script -->
```rhai
let due = table("order").where("status", "due");
#{ count: due.count(), rows: due.sort("total", "desc").list() }
```

```rust,ignore
let out = run_script(
    AGENT_SCRIPT.to_string(),
    resolver(&store),
    1,
    Writes::Denied("writing data is turned off".into()),
)
.await
.unwrap();
```

The limit is clamped to `MIN_LIMIT..=MAX_LIMIT` (1 to 50; `DEFAULT_LIMIT` is 5) and caps every
`list()`, so `rows` above holds one row while `count` is 2. This is an inspection surface, not a
bulk reader. Compile, runtime and backend errors all come back as the error string.

Vantage UI's MCP `run_data_script` tool is this function. Its writes stay `Denied` unless the user
turns on "Allow MCP agents to write data", a setting separate from the one that lets agents read.

## Query preview: `preview_script`

[`preview_script`](vantage_vista::preview_script) runs a script on a `Describe` host, so it can
build a handle but never read rows. The script must end on the handle. The resolved Vista renders
its query through `TableShell::preview_query`:

<!-- tested: rhai_guide::surfaces::preview -->
```rhai
table("order").where("status", "paid").sort("total", "desc")
```

The result is driver-shaped JSON. The memory store, for example, reports its driver, table,
conditions and order. One exception to "never reads": a `ref` step still reads the rows it starts
from, because the target's condition depends on them. When it finds none, the preview reports
`"query": null` with a note, because nothing would be queried.

## `modify:` scripts

A YAML spec's `modify:` script runs on the finished Vista, exposed as `self`. A `Describe` host
evaluates it with [`eval_modify_script`](vantage_vista::eval_modify_script):

<!-- tested: rhai_guide::surfaces::modify -->
```rhai
self.where("vip", true).sort("name", "desc")
```

When the script ends on a handle, that handle is resolved and becomes the table. When it ends on
anything else (a statement with `;`, say), the result of the last verb called on `self` is used;
otherwise `self` as given. `modify:` narrows a real table, so the Vista stays
writable. Augmentation sources finish the same way.

### Backend extensions

A backend can add its own verbs to handles through
[`TableShell::register_rhai_extensions`](vantage_vista::TableShell::register_rhai_extensions), and
its own variables through `TableShell::rhai_env`. SurrealDB adds its whole expression vocabulary,
plus `with_condition(expr)`, which narrows by a native SurrealQL expression that `where` can't
spell:

<!-- tested: vantage-surrealdb vista::factory::tests::modify_script_tweaks_built_vista_with_vendor_condition -->
```rhai
self.with_condition(ident("is_paying_client") == true)
   .sort("name", "asc")
```

and `me`, the current-record anchor for graph paths.

An extension verb changes a Vista in hand, not a list of steps, so it works on `self` only. On a
handle from `table(name)`, or after a `ref` step, it throws an error naming the verb. A backend
implements one with [`Handle::with_base_vista`](vantage_vista::Handle::with_base_vista). This
stand-in verb narrows by equality:

<!-- tested: rhai_guide::surfaces::extension_statements -->
```rust,ignore
fn only(h: &mut Handle, col: &str, value: Dynamic) -> Result<Handle, Box<EvalAltResult>> {
    let (col, value) = (col.to_string(), dynamic_to_cbor(value)?);
    h.with_base_vista("only", |vista| {
        vista.add_condition(col, FilterOp::Eq, value)
    })
    .map_err(|e| e.to_string().into())
}
```

`with_base_vista` works on a copy of the Vista and returns a new handle over it, so like every
other verb it leaves the handle it was called on alone: `let all = self; all.only("vip", true);
all` still ends on every client. (A backend whose shell can't be copied is changed in place.)

Statements on `self` accumulate, whichever verb they use:

<!-- tested: rhai_guide::surfaces::extension_statements -->
```rhai
self.only("vip", true);
self.only("name", "Cy");
```

leaves only Cy, and

<!-- tested: rhai_guide::surfaces::extension_statements -->
```rhai
self.only("vip", true);
self.sort("name", "desc");
```

leaves Cy and Ada, in that order. Each statement builds on the result of the one before, and the
script, ending on a statement, finishes with the last result. Chaining works too:
`self.only("vip", true).sort("name", "desc")` gives the same table. A handle from `table(name)`
doesn't accumulate: `let x = table("client").where("vip", true); x.sort("name"); x` ends on `x`
unsorted.

Backends without a scripting vocabulary (CSV, MongoDB, REST) keep the default no-op and get the
data vocabulary alone.

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

[`eval_ref_script`](vantage_vista::eval_ref_script) evaluates it. Drivers report the
`can_build_ref_via_script` capability when they support it.

## Augmentation sources

An augmentation joins a detail table onto each master row at the Dio layer (see
[Augmentation](../augmentation.md)). A script source narrows the detail table, given as `self`, for
the master `row`:

<!-- tested: rhai_guide::surfaces::augment -->
```rhai
self.where("client", row.id)
```

[`augment_source_closure(resolver, code)`](vantage_vista::augment_source_closure) turns the script
into the `Fn(&row, base) -> Vista` closure the Dio calls per row. It compiles once, on first use,
with the detail Vista's backend extensions registered.

## Computed columns

A `lazy:` column script runs on the shared background host, with no data vocabulary at all: no
`table(name)`, no handles, only `row`. It computes one value from one row, every time the Vista
reads that row. [Computed columns](./computed.md) covers it.

## Read-only hosts

`Terminals::Read` hosts read freely and refuse every write. Records still load, so a script can
read `row.status`, but saving throws:

<!-- tested: rhai_guide::surfaces::read_host -->
```rhai
let o = table("order").record("o1");
o.status = "void";
o.save()   // throws: this host only reads
```

Write verbs such as `delete` don't exist on a read host at all, and calling one is a "function not
found" error. Vantage UI uses read hosts for form `options:` scripts, and for action predicates,
which get `row` but no `table(name)`.

## Read-write hosts in an application

An application decides what its action scripts see; Vantage UI, for example, gives action bodies
and form `on_submit` scripts `table(name)` over the app's tables and, for row actions, `row` as a
[record](./records.md):

```rhai
let r = actions.cancel_order(row);
row.status = "cancelled";
row.cancellation_reason = r.reason;
row.save();
```

The resolver behind `table(name)` sends writes through each table's Dio, so open pages see them
at once, and applies the app's write rules (read-only tables, reference coercion, generated ids).
Form and wizard scripts also see Servo drafts, which have their own vocabulary
([Layers](./layers.md)). The action and form vocabularies themselves (`actions.*`, `notify`,
`form`) are documented in Vantage UI's skills.

## Faker sims

Sims are covered in [Faker Sims](./faker.md): they use this vocabulary over a memory store, with
`table()` naming the sim's own table, plus time and random verbs.
