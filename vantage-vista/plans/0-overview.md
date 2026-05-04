# Vista — multi-stage roadmap

`vantage-vista` is a crate housing `Vista`, the universal data handle that
drivers, scripting, UI, and agents consume. Vista is a richer,
schema-bearing, hook-aware first-class data model. It owns universal
metadata and delegates everything else to a per-driver `TableShell`.

## Architecture in one paragraph

`Vista` is a concrete struct (no consumer-facing trait surface). It owns
universal metadata — name, columns, references, capabilities, id column —
and a boxed `TableShell` (the executor). `TableShell` is the per-driver
trait. Drivers expose a `vista_factory()` inherent method that produces an
impl of `VistaFactory`, which constructs a Vista either from a typed
`Table<T, E>` or from a YAML spec. Both construction paths converge on the
same source-creation code, so post-construction Vista usage is fully
database-agnostic: the same code drives a Mongo Vista, a SQLite Vista, an
AWS Vista, or a CSV Vista.

`Vista` itself stores no condition state. `add_condition_eq(field, value)`
delegates to `TableShell::add_eq_condition`, which translates the
universal `(String, CborValue)` pair into the driver's native condition
type (`bson::Document` for Mongo, `Expression<AnyCsvType>` for CSV) and
mutates the wrapped `Table`'s condition list. Filtering happens
server-side wherever the backend supports it.

`Coop` is a separate crate (`vantage-coop`, stage 7) that wraps a factory
and adds caching, write routing, and live-event invalidation. Coop is
invisible to consumers — it only changes how the produced Vista behaves
at runtime.

## Crate layout

```
vantage-vista/src/
├── lib.rs              re-exports
├── vista.rs            the Vista struct + accessors + condition delegation
├── source.rs           TableShell trait — the driver contract
├── factory.rs          VistaFactory trait — YAML default impl + Extras assoc types
├── spec.rs             VistaSpec<T,C,R>, ColumnSpec<C>, ReferenceSpec<R>, NoExtras
├── column.rs           Vista's own column metadata + flag accessors
├── reference.rs        Reference + ReferenceKind
├── capabilities.rs     VistaCapabilities + PaginateKind
├── metadata.rs         VistaMetadata (builder for column/ref/id sets)
├── flags.rs            canonical flag string constants (ID, TITLE, …)
├── any_expression.rs   type-erased expression carrier (used by hooks, stage 6)
├── impls/              ValueSet trait impls forwarding Vista → TableShell
│   ├── readable_value_set.rs
│   ├── writable_value_set.rs
│   ├── insertable_value_set.rs
│   └── value_set.rs
└── mocks/
    └── mock_shell.rs          in-memory shell for tests
```

Driver crates each follow the same shape under `<driver>/src/vista/`:

```
vista/
├── mod.rs       re-exports + <Driver>::vista_factory() inherent impl
├── spec.rs      <Driver>TableExtras / <Driver>ColumnExtras / <Driver>VistaSpec
├── factory.rs   <Driver>VistaFactory + impl VistaFactory + spec→table helpers
├── source.rs    <Driver>TableShell + impl TableShell
└── cbor.rs      native ↔ CBOR bridge (Mongo only so far; CSV reuses its own
                  AnyCsvType→CborValue impl)
```

## Stage map

| Stage | File | Status |
|---|---|---|
| 1 | [Crate skeleton](1-skeleton.md) | Done |
| 2 | [First driver integration (CSV)](2-first-driver.md) | Done |
| 3 | [Universal YAML loader](3-yaml-loader.md) | Done |
| 4 | [Driver rollout](4-driver-rollout.md) | In progress — CSV + MongoDB done; SurrealDB / AWS / REST remain |
| 5 | [Portable conditions](5-conditions.md) | Partial — driver-typed `eq` shipped; portable operator vocabulary still pending |
| 6 | [Hooks + Rhai](6-hooks.md) | Not started |
| 7 | [vantage-coop crate](7-coop.md) | Not started |
| 8 | [vantage-ui migration](8-ui-migration.md) | Not started |
| 9 | [Decommission old types](9-decommission.md) | Not started |

MVP = stages 1–4 plus the eq-condition delegation that landed alongside
stage 4. Stages 5 (full operator vocabulary) onward are progressive
enhancement.

## What landed alongside stage 4

The MongoDB rollout doubled as the place where two cross-cutting decisions
got made. Both apply to every future driver:

- **Conditions delegate to the source, never live on Vista.**
  `Vista::add_condition_eq` calls into `TableShell::add_eq_condition`,
  which mutates the wrapped `Table`. This means filters push down to the
  backend (Mongo `find` filter, SQL `WHERE`, future REST query params)
  instead of being applied in memory after the fetch. Vista carries no
  condition state.
- **Per-column nested-path support via `column_paths`.**
  `MongoColumnBlock` introduced `nested_path: address.city`, and the
  source layer walks the path on read, rebuilds nested sub-documents on
  write, and uses dot-notation for filters. The pattern (`column_paths:
  IndexMap<String, Vec<String>>`) is documented in
  [step8-vista.md](../../docs4/src/new-persistence/step8-vista.md) and
  any backend with nested fields should reuse it.

## Conventions

- Each stage begins with a **discussion phase** — confirm interface and
  scope with the user before implementation. Questions deferred from
  earlier discussion are listed there.
- Each step has a checkbox; tick as you go.
- Each stage references items in `../../TODO.md`, `../../FINAL_TODO.md`,
  `../../PLAN_0_5.md` it subsumes; tick the parent entries once delivered.
- Tests use `Result<(), Box<dyn Error>>` (or `vantage_core::Result<()>`
  when no foreign error type is involved) so `?` replaces `.unwrap()`.

## Third-party developer guide

`docs4/src/new-persistence/step8-vista.md` is the high-level guide for
external driver authors adding Vista support. It documents the patterns
the in-tree drivers settled on, including the eq-condition delegation and
the `column_paths` mechanism.

## External references this overhaul tracks against

- `../../TODO.md` — multiple Architecture / MongoDB / SurrealDB / CI items
- `../../FINAL_TODO.md` — dataset surface, table-level operations, hooks
  framework, condition extensions, lazy expressions
- `../../PLAN_0_5.md` — column visibility, column (de)serialisation,
  table-level hooks, relationship eagerness
- `/Users/rw/Work/vantage-ui/app/todo/anytable-portable-conditions.md` —
  closed by stage 5 once the universal operator vocabulary lands
