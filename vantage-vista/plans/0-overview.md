# Vista — multi-stage roadmap

`vantage-vista` is a new crate housing `Vista`, the universal data handle
that drivers, scripting, UI, and agents consume. Vista is a richer,
schema-bearing, hook-aware, condition-policy-aware first-class data model.

## Architecture in one paragraph

`Vista` is a concrete struct (no consumer-facing trait surface). It owns
universal data — name, columns, references, condition policy, hooks,
capabilities — and a boxed `VistaSource` (the executor). `VistaSource` is
the per-driver trait. Drivers expose a `vista_factory()` that produces an
impl of `VistaFactory`, which constructs a Vista from typed `Table<T, E>`
or from YAML. Post-construction, Vista usage is fully database-agnostic:
the same code drives a Mongo Vista, a SQLite Vista, an AWS Vista, or a CSV
Vista.

`Coop` is a separate crate (`vantage-coop`) that wraps a factory and adds
caching, write routing, and live-event invalidation. Coop is invisible to
consumers — it only changes how the produced Vista behaves at runtime.

## Stage map

| Stage | File | Status |
|---|---|---|
| 1 | [Crate skeleton](1-skeleton.md) | Done |
| 2 | [First driver integration](2-first-driver.md) | Done |
| 3 | [Universal YAML loader](3-yaml-loader.md) | Not started |
| 4 | [Driver rollout](4-driver-rollout.md) | Not started |
| 5 | [Portable conditions](5-conditions.md) | Not started |
| 6 | [Hooks + Rhai](6-hooks.md) | Not started |
| 7 | [vantage-coop crate](7-coop.md) | Not started |
| 8 | [vantage-ui migration](8-ui-migration.md) | Not started |
| 9 | [Decommission old types](9-decommission.md) | Not started |

MVP = stages 1–4. Stages 5–9 are progressive enhancement.

## Conventions

- Each stage begins with a **discussion phase** — confirm interface and
  scope with the user before implementation. Questions deferred from
  earlier discussion are listed there.
- Each step has a checkbox; tick as you go.
- Each stage references items in `../../TODO.md`, `../../FINAL_TODO.md`,
  `../../PLAN_0_5.md` it subsumes; tick the parent entries once delivered.

## External references this overhaul tracks against

- `../../TODO.md` — multiple Architecture / MongoDB / SurrealDB / CI items
- `../../FINAL_TODO.md` — dataset surface, table-level operations, hooks
  framework, condition extensions, lazy expressions
- `../../PLAN_0_5.md` — column visibility, column (de)serialisation,
  table-level hooks, relationship eagerness
- `/Users/rw/Work/vantage-ui/app/todo/anytable-portable-conditions.md` —
  closed by stage 5
