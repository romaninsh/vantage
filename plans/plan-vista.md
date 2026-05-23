# Vista — consolidated roadmap

`vantage-vista` is a crate housing `Vista`, the universal data handle that drivers, scripting, UI,
and agents consume. Vista is a richer, schema-bearing, hook-aware first-class data model. It owns
universal metadata and delegates everything else to a per-driver `TableShell`.

## Architecture

`Vista` is a concrete struct (no consumer-facing trait surface). It owns universal metadata — name,
columns, references, capabilities, id column — and a boxed `TableShell` (the executor). `TableShell`
is the per-driver trait. Drivers expose a `vista_factory()` inherent method that produces an impl of
`VistaFactory`, which constructs a Vista either from a typed `Table<T, E>` or from a YAML spec. Both
construction paths converge on the same source-creation code, so post-construction Vista usage is
fully database-agnostic.

`Vista` stores no condition state. `add_condition_eq(field, value)` delegates to
`TableShell::add_eq_condition`, which translates the universal `(String, CborValue)` pair into the
driver's native condition type and mutates the wrapped `Table`'s condition list. Filtering happens
server-side wherever the backend supports it.

Capability fill-ins, caching, write routing, and live-event invalidation live in `vantage-diorama`.
From Vista's perspective, Diorama is just another consumer: it wraps a Vista, runs it through a
`Lens`, and exposes the result as a richer Vista plus a reactive `Scenery` surface.

### Crate layout

```
vantage-vista/src/
├── lib.rs              re-exports
├── vista.rs            Vista struct + accessors + condition delegation
├── source.rs           TableShell trait — driver contract
├── factory.rs          VistaFactory trait — YAML default impl + Extras assoc types
├── spec.rs             VistaSpec<T,C,R>, ColumnSpec<C>, ReferenceSpec<R>, NoExtras
├── column.rs           Vista column metadata + flag accessors
├── reference.rs        Reference + ReferenceKind
├── capabilities.rs     VistaCapabilities + PaginateKind
├── metadata.rs         VistaMetadata (builder for column/ref/id sets)
├── flags.rs            canonical flag string constants (ID, TITLE, …)
├── any_expression.rs   type-erased expression carrier (used by hooks, stage 6)
├── impls/              ValueSet trait impls forwarding Vista → TableShell
└── mocks/
    └── mock_shell.rs   in-memory shell for tests
```

Driver crates follow the same shape under `<driver>/src/vista/`:

```
vista/
├── mod.rs       re-exports + <Driver>::vista_factory()
├── spec.rs      <Driver>TableExtras / ColumnExtras / VistaSpec
├── factory.rs   <Driver>VistaFactory + impl VistaFactory + spec→table helpers
├── source.rs    <Driver>TableShell + impl TableShell
└── cbor.rs      native ↔ CBOR bridge (where needed)
```

### Stage map

| Stage                  | Status      |
| ---------------------- | ----------- |
| 1 — Skeleton           | Done        |
| 2 — First driver (CSV) | Done        |
| 3 — YAML loader        | Done        |
| 4 — Driver rollout     | Mostly done |
| 5 — Conditions         | Partial     |
| 5b — Query controls    | Not started |
| 6 — Hooks + Rhai       | Not started |
| 8 — UI migration       | Not started |
| 9 — Decommission       | Not started |

### Conventions

- Each stage begins with a **discussion phase** — confirm interface and scope before implementation.
- Each step has a checkbox; tick as you go.
- Tests use `Result<(), Box<dyn Error>>` or `vantage_core::Result<()>`.
- External developer guide: `docs4/src/new-persistence/step8-vista.md`.

---

## Stage 1 — Crate skeleton (Done)

Created `vantage-vista` crate with type definitions and trait surfaces. No execution logic or driver
implementations.

**Key decisions:**

- `Vista` struct: name, columns (`IndexMap`), references, capabilities, id_column, title_columns,
  source (`Box<dyn TableShell>`). No condition state.
- `TableShell` trait: async CRUD + count; sync capabilities. `vantage_core::Result` everywhere.
- `VistaFactory`: `from_yaml` on the trait; `from_table` as per-driver inherent method to avoid
  dependency cycle.
- `VistaCapabilities`: explicit fields (`can_count`, `can_insert`, `can_update`, `can_delete`,
  `can_subscribe`, `can_invalidate`, `paginate_kind`).
- `AnyExpression` moved from `vantage-expressions` into `vantage-vista`; re-export preserved.
- Carrier types: `&CborValue` for ids, `&Record<CborValue>` for records.

**References:** Subsumes PLAN_0_5 §1 (column visibility), §2 (column serialisation), §3 (hooks —
preparation only).

---

## Stage 2 — First driver / CSV (Done)

Wired CSV end-to-end: typed `Table<Csv, E>` → Vista → real `list` query. Validated trait shape
against a real backend.

**Key decisions:**

- `vantage-csv` gets `vista` cargo feature; existing TableSource path unaffected.
- CBOR translation reuses existing `From<AnyCsvType>` impls.
- CSV is read-only: `can_count: true`, writes return `Unsupported`.
- bakery_model3's CSV CLI branch converted to drive a Vista.

**References:** Subsumes TODO "Architecture: Make ImTable generic over Value" (partial).

---

## Stage 3 — YAML loader (Done)

YAML → Vista construction. Universal vocabulary parsed by `vantage-vista`; driver-specific extras
via three generic parameters on `VistaSpec`.

**Key decisions:**

- `VistaSpec<T, C, R>` — universal fields only (`name`, `datasource`, `id_column`, `columns`,
  `references`). Title membership is a column flag only.
- Driver-specific YAML under top-level keys named by the driver (e.g. `csv:`), both at table and
  column level.
- Flag vocabulary is open `Vec<String>`; constants in `vantage_vista::flags` (`ID`, `TITLE`,
  `SEARCHABLE`, `MANDATORY`, `HIDDEN`).
- Reference kinds: `has_one`, `has_many`, `has_foreign`. Sugar form supported.
- Errors wrapped `serde_yaml_ng::Error` → `vantage_core::Error`. Driver blocks use
  `deny_unknown_fields`.

**Remaining:**

- [ ] Document YAML schema in `SCHEMA.md` (deferred to stage 4 completion).

**References:** Subsumes PLAN_0_5 §1 + §2. Replaces vantage-ui's per-driver column-extras pattern.

---

## Stage 4 — Driver rollout (Mostly done)

### Shipped drivers

| Driver                       | Factory | Shell | Tests        | Capabilities               |
| ---------------------------- | ------- | ----- | ------------ | -------------------------- |
| `vantage-csv`                | ✅      | ✅    | ✅           | `count` (read-only)        |
| `vantage-mongodb`            | ✅      | ✅    | ✅           | full CRUD + count          |
| `vantage-surrealdb`          | ✅      | ✅    | ✅           | full CRUD + count          |
| `vantage-sql/sqlite`         | ✅      | ✅    | ✅           | full CRUD + count          |
| `vantage-sql/postgres`       | ✅      | ✅    | ✅           | full CRUD + count          |
| `vantage-sql/mysql`          | ✅      | ✅    | ✅           | full CRUD + count          |
| `vantage-api-client/rest`    | ✅      | ✅    | ✅           | `count` (read-only)        |
| `vantage-api-client/graphql` | ✅      | ✅    | example only | `count` (read-only)        |
| `vantage-log-writer`         | ✅      | ✅    | ✅           | `insert` only (write-only) |

### Remaining drivers

| Driver                             | Status      | Open questions                                                                                                                                                                              |
| ---------------------------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vantage-aws` (account + dynamodb) | Not started | Does `AwsAccount` need a Vista? Composite-key handling. Where `array_key:service/target` addressing moves. `narrow_via` into driver extras. AWS protocol family per-service or one factory? |
| `vantage-redb`                     | Not started | Conditions other than id need client-side scan — reject or Diorama fill? Range queries on Vista? Schema: opaque bytes → what `column_types()` returns?                                      |
| `vantage-api-pool`                 | Not started | Transparent (PoolApi wraps RestApi Vista) or needs its own factory? Lean: transparent — rate-limit/retry/cache belong in Diorama `Lens`.                                                    |

### Cross-cutting gaps

- **`paginate_kind`**: universally `None`. Waits for Stage 5b (`set_pagination`).
- **`can_subscribe`**: universally `false`. Wiring belongs in `vantage-diorama`.
- **`add_raw_condition`**: role shrunk after `HasForeign` retirement. REST still overrides; may be
  redundant after Stage 9.

### What landed alongside Stage 4

- **Row-based traversal**: `Vista::get_ref(relation, &row)` on every same-driver shell. Join field
  reads from the known parent row; one eq-condition pushes to target.
- **`Vista::with_foreign(name, kind, closure)`** replaces retired `HasForeign`. Closure stored,
  never invoked at registration.
- **`Vista::with_id(id)`** — convenience narrowing for id-only workflows.
- **`Vista::list_references()`** — combines foreign resolvers, YAML refs, typed refs.
- **Per-column nested-path support** via `column_paths` (MongoDB pattern, reusable).
- **Conditions delegate to source, never live on Vista.** Push-down is automatic.

### Remaining items

- [ ] AWS / Redb / api-pool driver implementation (see per-driver questions above)
- [ ] Cross-driver `get_ref` integration test (e.g. Postgres → Mongo via `with_foreign`)
- [ ] YAML inventory loader for cross-driver refs — document in `step8-vista-integration.md`
- [ ] GraphQL: integration tests under `tests/` (currently only example coverage)
- [ ] REST: capability honesty — which HTTP verbs map to insert/update/delete
- [ ] SurrealDB: `can_subscribe` + LIVE query wiring (into Diorama)
- [ ] SQL drivers: advertise `paginate_kind: Offset` once Stage 5b lands

**References:** Subsumes TODO "MongoDB/CSV CBOR fidelity", "Add sample CSV table". Touches TODO
"Drop String variant from MongoId", "Wire real LIVE query support".

---

## Stage 5 — Portable conditions + per-column policy (Partial)

### What shipped (stage 4 spillover)

- `Vista::add_condition_eq(field, CborValue) -> Result<()>` — universal entry point; delegates to
  source.
- `TableShell::add_eq_condition(&mut self, field, value)` — driver contract; default returns
  `Unimplemented`. CSV, Mongo, SQL, SurrealDB, REST all override.
- Vista carries no condition state — every call mutates the wrapped `Table`; push-down is automatic.

### Discussion phase (open)

- [ ] Per-column policy: runtime metadata only? (Lean: yes)
- [ ] Operator vocabulary: fixed `Op` enum (`Eq`, `Ne`, `Lt`, `Lte`, `Gt`, `Gte`, `Like`, `In`,
      `IsNull`, `IsNotNull`)? (Lean: fixed enum)
- [ ] Default policy when YAML doesn't declare: type-driven defaults?
- [ ] Composition: flat AND only for v1, document path to nested?
- [ ] Failure mode for unsupported op: hard error? (Lean: yes, at `add_condition` call site)
- [ ] REST per-column server-capability declaration?
- [ ] Removal: handle-based (`ConditionHandle` → `remove_condition`)? (Lean: handles, mirroring
      `temp_add_condition`)
- [ ] `add_condition_eq` stays as thin wrapper for common case?

### Scope

In:

- `Op` enum (universal operator vocabulary)
- Per-column condition policy (runtime metadata)
- `Vista::add_condition(field, op, value) -> Result<ConditionHandle>`
- `Vista::remove_condition(handle)`
- Driver-side `TableShell::add_condition` extending `add_eq_condition`
- Default type-driven policy in `Column`
- YAML-time policy override per column

Out: Nested AND/OR (v2), search across SEARCHABLE columns (stage 5b), hook-mediated condition
rewriting (stage 6).

### Plan

- [ ] Discuss open questions above
- [ ] Define `Op` enum
- [ ] Define `ConditionPolicy` per column (set of allowed ops)
- [ ] Default-policy table by column type
- [ ] YAML schema: per-column `conditions: [eq, like, ...]` override
- [ ] `Vista::add_condition` / `remove_condition` with handle; reroute `add_condition_eq`
- [ ] `TableShell::add_condition` trait method; CSV + Mongo override
- [ ] Remaining drivers' eq + full operator translations
- [ ] REST per-column capability declaration in `rest:` extras
- [ ] Integration test: master/detail traversal on sqlite/surreal/mongo/rest

**References:** Closes `vantage-ui/app/todo/anytable-portable-conditions.md`. Subsumes TODO
"Decouple column_table_values_expr from ExprDataSource". Touches TODO "Condition::or() shouldn't be
limited to two arguments".

---

## Stage 5b — Query controls: sort, paginate, search, aggregates (Not started)

Vista today exposes schema, eq-narrowing, and `get_count`. Sort, pagination, search, and aggregates
have no surface yet.

Architecturally pairs with `vantage-diorama`: every method returns `Unsupported` on drivers that
can't push down, and a Diorama-wrapped Vista fills the gap client-side.

### Discussion phase

- [ ] Sort: flat `add_order(field, direction)`? (Lean: flat, field name only for v1)
- [ ] Removable sort handles? (Lean: yes, same pattern as conditions)
- [ ] `set_pagination`: reuse `vantage_table::Pagination`? (Lean: yes)
- [ ] Search: `add_search(&str)` auto-building OR across SEARCHABLE columns? (Lean: yes, mutates)
- [ ] Aggregate return type: `CborValue`? (Lean: yes)
- [ ] Aggregate column reference: by name only?
- [ ] Computed/expression columns: defer to stage 6 hooks? (Lean: yes)

### Scope

In:

- `Vista::set_pagination` / `pagination()` pair; `TableShell::set_pagination` / `get_pagination`
- `Vista::add_order` / `temp_add_order` / `temp_remove_order` with `OrderHandle`
- `Vista::add_search(value)` backed by SEARCHABLE flag; `TableShell::add_search` (default
  `Unsupported`)
- `Vista::get_sum(field)` / `get_max(field)` / `get_min(field)` → `Result<CborValue>`
- Capability flags: `can_paginate_native`, `can_sort_native`, `can_search_native`,
  `can_aggregate_native`

Out: Computed columns (stage 6 hooks), GROUP BY / HAVING (post stage 9), UNION / EXCEPT,
cross-reference search, `get_avg`.

### Plan

- [ ] Discuss open questions above
- [ ] `Vista::set_pagination` / `pagination()` + `TableShell` hooks
- [ ] `Vista::add_order` / temp variants + `TableShell::add_order` (default `Unsupported`); CSV +
      Mongo override
- [ ] `Vista::add_search` + `TableShell::add_search` (default `Unsupported`); Mongo → `$regex`, CSV
      → column scan
- [ ] `Vista::get_sum/max/min` + `TableShell::get_vista_sum/max/min` (default `Unsupported`); CSV +
      Mongo override
- [ ] Capability-flag vocabulary; update `VistaCapabilities`
- [ ] Integration tests: CSV + Mongo + `Unsupported` stub assertion
- [ ] Cross-link to Diorama: every `Unsupported` path is a Diorama fill-in target

**References:** Subsumes FINAL_TODO "Search across all columns". Touches TODO "Move
get_count/get_sum/get_max/get_min off SelectableDataSource".

---

## Stage 6 — Hooks lifecycle + Rhai integration (Not started)

> **Merges PLAN_0_5.md §3 (table-level hooks)**

Add hook support: pre/post lifecycle callbacks for read, insert, update, delete. Hooks declared in
YAML run as Rhai scripts; hooks added in Rust use a typed trait.

### Discussion phase

- [ ] Read-only observers vs mutating interceptors vs both? (Lean: both, `Observer::observe(&Ctx)`,
      `Interceptor::intercept(&mut Ctx) -> Outcome`)
- [ ] Hook outcome: `Continue`, `Skip`, `Reject(error)`?
- [ ] Lifecycle points: `before_select`, `after_select`, `before_insert`, `after_insert`,
      `before_update`, `after_update`, `before_delete`, `after_delete`
- [ ] Hook ordering: registration order? priority field?
- [ ] Rhai context bindings: which Vista APIs exposed to scripts?
- [ ] Where hooks live: typed `HookCollection` field or separate `HookRegistry`?
- [ ] Programmatic Rust hook registration post-construction (vantage-ui need)?

### Scope

In:

- `Hook` trait (or split `Observer` / `Interceptor`)
- `HookCtx` struct with read/write access per lifecycle
- `Outcome` enum
- YAML `hooks:` block → Rhai scripts
- Rhai engine + Vista context bindings
- Programmatic `Vista::with_hook(...)`
- Integration test: Rhai `before_insert` script rejecting a record

Out: Backend-specific query rewriting via hooks (deferred, possibly stage 6.5), soft-delete
extension (follow-up using this surface).

### Plan

- [ ] Discuss: hook signature, outcome semantics, lifecycle set, ordering
- [ ] Define `Hook` trait(s) and `Outcome` enum
- [ ] Define `HookCtx` per lifecycle
- [ ] Add Rhai dependency to `vantage-vista`
- [ ] Bind Vista record / condition / reject APIs into Rhai engine
- [ ] YAML `hooks:` parser → compile to Rhai AST at construction
- [ ] `Vista::with_hook(...)` for Rust-side registration
- [ ] Hook execution wired into TableShell CRUD calls (in `vantage-vista`, not drivers)
- [ ] Integration test with Rhai validation hook
- [ ] Document Rhai context API in `HOOKS.md`

**References:** Subsumes PLAN_0_5 §3 "Table-level hooks", FINAL_TODO "Hooks / extensions framework".
Touches FINAL_TODO "Lazy expressions / post-fetch transforms" (`AfterQuery` → `after_select` hook).

---

## Stage 8 — vantage-ui migration (Not started)

Migrate `vantage-ui` to consume `Vista` instead of the older type-erased wrapper. Eliminates
parallel column-threading, JSON↔CBOR adapter, `is_api_backed` flag, and AWS-only condition
asymmetry.

### Discussion phase

- [ ] Parallel period (both old and new coexist) vs hard cutover on 0.5 branch?
- [ ] Features depending on old wrapper internals that need re-exposure?
- [ ] Driver registration: `Box<dyn VistaFactory>` per datasource, or per-driver concrete factory?
- [ ] Live-update wiring: Diorama-backed Vistas where reactive, plain otherwise?
- [ ] Master/detail: confirm portable conditions (stage 5) replace AWS-only path
- [ ] Storybook implications: mock factories for fixtures?

### Scope

In:

- Replace 4× `build_*_table` dispatch with single `factory.from_yaml(yaml)?` call per config
- Drop `EntityBackend.columns` parallel field; read from Vista
- Delete `app/src/backend/json_cbor_adapter.rs`
- Delete `components/src/schema_column.rs` shim
- Replace `is_api_backed` with `vista.capabilities()` queries
- Replace AWS-only master/detail with portable conditions
- Wire Rhai hooks
- Wire reactive grid through Diorama-backed Vista where applicable

Out: Decommissioning old types in vantage workspace (stage 9), new vantage-ui features.

### Plan

- [ ] Discuss migration strategy, driver registration, Diorama split, storybook impact
- [ ] Add `vantage-vista` and per-driver deps to vantage-ui
- [ ] Replace `build_sqlite/surreal/api/aws_table` with `Box<dyn VistaFactory>` dispatch
- [ ] Drop `JsonToCborAdapter`
- [ ] Drop `SchemaColumn` and `schema_columns()` mirror
- [ ] Drop `EntityBackend.columns`; grid reads from Vista
- [ ] Replace `is_api_backed` with capability queries
- [ ] Update master/detail to use `vista.add_condition(...)`
- [ ] Wire Diorama-backed Vistas where reactive UI needed
- [ ] Update inventory YAML schema if needed
- [ ] Close `vantage-ui/app/todo/anytable-portable-conditions.md`
- [ ] Smoke test: all bakery fixtures, grids, master/detail, search, pagination

**References:** Closes `vantage-ui/app/todo/anytable-portable-conditions.md`. Subsumes
`EntityBackend.columns` workaround, `JsonToCborAdapter`, `SchemaColumn` shim, `is_api_backed` flag,
4× repeated YAML mapping.

---

## Stage 9 — Decommission old types (Not started)

Remove the old type-erased wrapper, live-table types, and related shims. Final cleanup pass.

### Discussion phase

- [ ] Confirm Vista feature parity — every old-wrapper use case has Vista equivalent
- [ ] Confirm vantage-ui fully migrated; no external consumers of old types
- [ ] Single-cut at 0.5 or deprecate-and-warn for one cycle?
- [ ] `vantage-live` fate: fully removed (superseded by Diorama) or thin re-export shim?
- [ ] `bakery_model4` (excluded from workspace) — bring in or leave excluded?

### Scope

In:

- Delete `vantage-table/src/any.rs`
- Delete legacy `Table::get_ref/get_ref_as/get_subquery_as` and
  `Reference::resolve_as_any/build_target` (superseded by `resolve_from_row`)
- Rewrite REST/GraphQL `get_ref` to route through `Reference::resolve_from_row` directly (drop
  `AnyTable` carrier)
- Decide fate of `TableShell::add_raw_condition`
- Delete old `TableLike` trait family
- Delete or shrink `vantage-live` (logic → `vantage-diorama`)
- Delete legacy `AnyTable` trait at `vantage/src/sql/table.rs`
- Restore disabled tests as Vista tests
- Update `bakery_model3`/`bakery_model4` to Vista
- Sweep examples for old-type references

Out: Re-architecting features not in stages 1–8.

### Plan

- [ ] Discuss parity audit, deprecation timing, vantage-live fate
- [ ] Audit Vista coverage; produce parity checklist
- [ ] Delete legacy `Table::get_ref` / `Reference::resolve_as_any` / `build_target` from
      `vantage-table`
- [ ] Rewrite REST/GraphQL `get_ref` → `Reference::resolve_from_row`
- [ ] Delete `vantage-table/src/any.rs`
- [ ] Delete/replace `vantage-table/src/traits/table_like.rs`
- [ ] Delete legacy `AnyTable` trait at `vantage/src/sql/table.rs`
- [ ] Decide `TableShell::add_raw_condition` fate
- [ ] Delete/shrink `vantage-live`
- [ ] Restore `vantage-table/tests/table_like.rs` as Vista tests
- [ ] Convert `MockTableSource` to `Value = ciborium::Value`
- [ ] Make `ImTable/ImDataSource` generic over `Value`
- [ ] Update `bakery_model3` + `bakery_model4`
- [ ] Sweep `example_*` crates
- [ ] Update CHANGELOG, README, ARCHITECTURE

**References:** Closes TODO "AnyTable CBOR-swap follow-up" subtree, "Architecture: Make ImTable
generic over Value". Removes legacy `vantage/src/sql/table.rs::AnyTable` trait.

---

## Appendix: REST API Vista deferred features

From `vantage-api-client/TODO.md`. Features that vantage-ui's `inventory::TableConfig` carries but
`RestApiVistaSpec` does not yet.

### Obsolete `AnyTable` in the Vista path

`RestApiTableShell::get_ref` has a YAML path (clean — resolver returns `Vista` directly) and a
typed-Rust fallback (`AnyTable → AnyTableShell::into_vista`). The fallback exists because
`TableLike::get_ref` is contractually `Result<AnyTable>` — `vantage-table` can't return `Vista`
without inverting the dependency.

Path to remove:

1. Add Rust-native relations at Vista layer: `Vista::with_many(rel, fk, Fn() -> Vista)`
2. Demote `Table::with_many/with_one/with_foreign` to legacy
3. Migrate all examples to Vista-layer API
4. Drop `TableLike::get_ref` fallback
5. Delete `AnyTableShell`

Larger refactor; tracked as Stage 9 work.

### Replace `Expression<CborValue>` with `ApiCondition`

`RestApi::Condition = Expression<CborValue>` is overkill for REST — only eq-conditions + deferred-FK
are used. Mirror CSV's focused condition type:

```rust
pub enum ApiCondition {
    Eq { field: String, value: ApiValue },
}
pub enum ApiValue {
    Scalar(CborValue),
    Deferred(DeferredFkFn),
}
```

~150–200 lines crate-local rewrite. No external behaviour change. Worth doing before more REST
features accrete on the Expression shape.

### Ship default `Renderer` in `vantage-cli-util`

`vantage_cli_util::vista_cli` defines a `Renderer` trait but no default impl. Both
`dynamo-single-table.rs` and `jsonplaceholder*.rs` carry ~80 lines of near-identical tab-printing
logic. Build `DefaultRenderer` impl; consumers shrink to one-liner. Net negative LoC.

### Validation rules

vantage-ui ships `rules: { email: true, unique: true }` per column for client/server validation.
Open question: belong in `VistaSpec` (every driver honours them) or UI-layer extension?

### Static params (`params`)

vantage-ui uses `params: { eq: { archived: false } }` for default filter conditions baked into a
Vista. Out of scope until AWS-style APIs are wired.

### `narrow_via` on references

For AWS-style APIs filtered by string-prefix on parent id. Not needed by jsonplaceholder; revisit
for CloudWatch / S3 / IAM.

### Rhai expression columns

vantage-ui supports `expressions: { full_name: "name + ' ' + last_name" }` for computed columns.
Plausibly a Vista-layer feature via post-processing. Needs design: sandboxing, perf (eval per row),
dependency implications. Overlaps with Stage 6 hooks.

### Declined / relocated

- **Datasource fields** (`auth`, `response_shape`, `pagination`, `base_url`): describe the
  datasource, not individual tables. Stay out of table YAML.
- **UI rendering hints** (`color`, `labels`, `link`, `width`, `pin`): pure rendering hints. Belong
  in UI-layer extension, part of Stage 8 migration.
