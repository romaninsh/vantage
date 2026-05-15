# Vista YAML — deferred features

Features that vantage-ui's `inventory::TableConfig`
(`/Users/rw/Work/worktrees/vantage-ui/witty-burrow/vantage-ui/inventory/src/table.rs`)
carries but `RestApiVistaSpec` does not. Each one needs a design call
before adoption.

# Architectural follow-ups

Items below are not YAML features — they're cleanups that came out of
implementing the YAML factory and are worth a dedicated PR each.

## Obsolete `AnyTable` in the Vista path

`RestApiTableShell::get_ref` has two branches: the YAML path (resolver
returns `Vista` directly — clean) and the typed-Rust fallback
(`self.table.get_ref(relation)` returns `AnyTable`, which we then wrap
via `AnyTableShell::into_vista`). The fallback exists because
`TableLike::get_ref` is contractually `Result<AnyTable>` — and
`vantage-table` can't return `Vista` directly without inverting the
crate dependency (`vantage-vista` already depends on `vantage-table`).

Path to remove it:

1. Add Rust-native relation declarations at the Vista layer:
   `Vista::with_many(rel, fk, Fn(...) -> Vista)`, paralleling the
   YAML registry the shell already maintains.
2. Demote `Table::with_many` / `with_one` / `with_foreign` to legacy.
   They stay around for compat but stop being the entry point for
   declaring relations.
3. Move all the user-facing examples (jsonplaceholder, bakery models,
   dynamo example) to the Vista-layer API.
4. Drop the `TableLike::get_ref` fallback in `RestApiTableShell` —
   YAML refs and Rust refs both route through the same Vista-layer
   registry.
5. Delete `AnyTableShell` (only exists to bridge the legacy path).

Bigger refactor than today's; `AnyTable` survives across drivers
(CSV, SQL, Mongo, etc.) until those examples migrate too.

## Replace `Expression<CborValue>` with a custom `ApiCondition`

`RestApi::Condition = vantage_expressions::Expression<CborValue>` —
inherited from when the crate mirrored SQL-shaped expressions. For
REST we only ever build eq-conditions plus the deferred-FK variant,
and the URL builder peels them by template-string matching
(`"{} = {}"`) and nested `ExpressiveEnum` indexing. That's overkill.

Mirror what `vantage-csv` does — ship a focused condition type:

```rust
pub enum ApiCondition {
    Eq { field: String, value: ApiValue },
}

pub enum ApiValue {
    Scalar(CborValue),
    /// Resolves at fetch time — used by YAML / `with_one`-style
    /// FK conditions that need the parent's value.
    Deferred(DeferredFkFn),
}
```

What changes:

- `operation.rs` simplifies — `eq_condition` builds the enum directly;
  `condition_to_query_param` matches on `ApiCondition::Eq` instead of
  template strings.
- `endpoint_url` / `build_query_string` / `related_in_condition` /
  `resolve_deferreds` in `api.rs` switch to the typed enum.
- `vantage-expressions` dependency may drop from `vantage-api-client/Cargo.toml`
  (verify — check for stragglers via `ExprDataSource` impl).
- `RestApiTableShell::add_eq_condition` keeps its public signature
  (`field, &CborValue`); only the typed thing flowing into
  `self.table.add_condition` changes shape.
- Tests in `operation.rs` re-stated against the enum.

~150–200 lines of crate-local rewrite. Strictly a clarification — no
external behaviour change. Worth doing before more REST-specific
features (e.g. `narrow_via`) accrete on the Expression shape.

## Ship a default `Renderer` in `vantage-cli-util`

`vantage_cli_util::vista_cli` defines a `Renderer` trait
(`render_list` + `render_record`) but no default impl, so every
consumer rolls its own. Today both
`vantage-aws/examples/dynamo-single-table.rs` and the two
`jsonplaceholder*.rs` examples carry ~80 lines of near-identical
tab-printing + title-field + relations-footer logic. Pure
duplication.

What to build:

```rust
// vantage-cli-util/src/vista_cli.rs
pub struct DefaultRenderer;

impl Renderer for DefaultRenderer {
    fn render_list(&self, vista: &Vista, records: &..., column_override: Option<&[String]>) {
        // delegate to `render_records_typed` (already in cli-util)
        // honour title fields / column_override / id column.
    }
    fn render_record(&self, vista: &Vista, id: &str, record: &..., relations: &[String]) {
        // title fields → divider → remaining fields → relations footer
    }
}
```

Consumers shrink to:

```rust
use vantage_cli_util::vista_cli::{self, DefaultRenderer};
vista_cli::run(&factory, &DefaultRenderer, &args).await?;
```

Net negative LoC across the workspace. The bakery `cli-vista.rs`
example already uses `render_records` for list mode, so the styled
output is known good for `Record<CborValue>`. The only visible
behaviour change is that the dynamo example (currently bare
`\t`-separated) switches to comfy-table styling — almost certainly
an improvement. Examples that want different shapes keep rolling
their own.

vantage-ui ships per-column `rules: { email: true, unique: true }` for
client/server validation.

```yaml
- name: email
  type: string
  rules: { email: true, unique: true }
```

Open question: do validation rules belong in `VistaSpec` (every driver
honours them) or in a UI-layer extension? Schema validation is a
cross-cutting concern; centralising it in Vista keeps drivers
consistent but expands the spec surface.

## Static params (`params`) — out of scope

vantage-ui uses `params: { eq: { archived: false } }` for AWS
operations that need a mandatory request body, plus to bake default
filter conditions into a Vista.

```yaml
params:
  eq:
    archived: false
```

Maps roughly to "default eq-conditions baked into the Vista at build
time". Worth adopting once we wire AWS-style APIs through this
factory.

## `narrow_via` on references — out of scope

For AWS-style APIs filtered by a string-prefix on the parent's id —
not a foreign key — vantage-ui's `ReferenceDef` has `narrow_via`:

```yaml
references:
  events:
    table: aws_log_events
    kind: has_many
    foreign_key: logGroupName
    narrow_via: logGroupNamePrefix
```

The parent's `logGroupName` value submits as `?logGroupNamePrefix=…`
on the child request rather than `?logGroupName=…`. jsonplaceholder
doesn't need it; revisit when wiring CloudWatch / S3 / IAM through
this factory.

## Rhai expression columns — yes for consideration

vantage-ui supports `expressions: { full_name: "name + ' ' + last_name" }`
for server-side computed columns:

```yaml
expressions:
  full_name: "name + ' ' + last_name"
```

Plausibly a Vista-layer feature — any driver could honor it via
post-processing the row stream. Needs design before adoption:
embedded scripting language has security (sandboxing), perf (eval per
row), and `vantage-vista` dependency-graph implications.

## Datasource fields in table YAML — declined

`auth`, `response_shape`, `pagination`, `base_url` describe the
datasource, not individual tables. vantage-ui already keeps them in a
separate `datasource: <name>.yaml`. They stay out of table YAML; the
`RestApi` constructor consumes them in Rust before the factory runs.

## UI rendering hints — relocate to UI layer

Per-value `color` / `labels`, column `link` / `width` / `pin` /
`label` / `tooltip` are pure rendering hints. They belong in a UI-side
extension (a sibling spec or overlay), not at the Vista layer. Their
new home is part of the vantage-ui migration that motivates this
factory.
