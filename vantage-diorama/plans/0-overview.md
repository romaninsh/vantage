# Diorama — multi-stage roadmap

`vantage-diorama` is a new crate housing `Lens` (the cache-and-callback
apparatus), `Dio` (the per-entity wrapping that lets cheap Vistas and
Sceneries be spawned), and the three Scenery surfaces (`TableScenery`,
`RecordScenery`, `ValueScenery`) that reactive UIs bind to.

## Architecture in one paragraph

A `Lens` is built once per application with a cache backend, a set of
lifecycle callbacks (`on_start`, `on_refresh`, `on_write`, `on_event`,
`on_query`), and default policies (TTL, refresh interval, write-queue
capacity). After `.build()`, `lens.make_dio(vista)` is the cheap factory:
hand it any Vista (typically a low-capability one — a CSV, a paginated
REST endpoint, an unsortable DynamoDB table) and you get back a `Dio`
that owns a cache namespace under the lens's cache backend plus the
per-entity machinery (a write-queue worker, a refresh task, an event
bus). From a `Dio` you spawn two consumer surfaces: `dio.vista()` returns
a richer Vista whose `TableShell` routes through the Dio's machinery
(reads served from cache, writes routed through `on_write`, capabilities
re-derived to reflect what callbacks add); `dio.table_scenery()` /
`dio.record_scenery(id)` / `dio.value_scenery()` return reactive
subscriptions that bump a `watch::Receiver<Generation>` on every
underlying change. The Lens is the long-lived shared infrastructure; the
Dio is the per-entity binding; the Vista and Sceneries are the
short-lived consumer handles.

`Diorama::overlay(a, b)` and `Diorama::merge(a, b)` are composition
primitives that produce a single Vista-shaped value from two underlying
Vistas, with the capability union reported correctly. The Lens treats
the composed Vista like any other input — composition is orthogonal to
caching.

## Crate layout

```
vantage-diorama/src/
├── lib.rs                    re-exports
├── lens/
│   ├── mod.rs                Lens, LensBuilder
│   ├── callbacks.rs          callback type aliases + boxing helpers
│   ├── defaults.rs           LensDefaults
│   └── build.rs              build() and validation
├── dio/
│   ├── mod.rs                Dio, DioInner
│   ├── shell.rs              DioShell : TableShell
│   ├── worker.rs             write queue worker
│   ├── refresh.rs            refresh task
│   ├── event_bus.rs          DioEvent + broadcast wiring
│   └── hot_tier.rs           moka wrapper
├── scenery/
│   ├── mod.rs                trait re-exports
│   ├── table.rs              TableScenery + TableSceneryBuilder + state
│   ├── record.rs             RecordScenery
│   ├── value.rs              ValueScenery
│   └── enriched_record.rs    EnrichedRecord + RowStatus
├── composition/
│   ├── mod.rs                Diorama:: prefix entry points
│   ├── overlay.rs            OverlayVista
│   └── merge.rs              MergeVista
├── ops/
│   ├── write_op.rs           WriteOp enum
│   ├── query_descriptor.rs   QueryDescriptor (for on_query)
│   └── change_event.rs       ChangeEvent (for on_event)
└── error.rs                  LensBuildError, DioError
```

This mirrors `vantage-live`'s `live_table/` for the worker/event-consumer
split and follows the workspace convention of putting trait impls under
`impls/` subdirs where they're load-bearing (e.g.
`dio/impls/table_shell.rs` for the `TableShell` impl on `DioShell`).

The role-facing documentation lives next to the crate root:

```
vantage-diorama/
├── README.md             entry door
├── README_rust_dev.md    business logic, API, CLI
├── README_lens.md        configuring a Lens, scenarios
├── README_ui.md          GPUI integration, Scenery bindings
├── README_models.md      model authors (bakery_model3 shape)
├── README_datasource.md  Vista driver authors
├── ARCHITECTURE.md       internal protocols, structs, concurrency model
└── plans/
    └── *.md              this directory
```

## Stage map

| Stage | File | Status |
|---|---|---|
| 1 | [Crate skeleton](1-skeleton.md) | Done |
| 1b | [Schema-on-source refactor](1b-schema-on-source.md) | Done |
| 2 | [CSV walkthrough (first end-to-end)](2-csv-walkthrough.md) | Not started |
| 3 | [Write queue + refresh scheduler](3-write-and-refresh.md) | Not started |
| 4 | [Event bus + on_event](4-event-bus.md) | Not started |
| 5 | [TableScenery](5-table-scenery.md) | Not started |
| 6 | [RecordScenery](6-record-scenery.md) | Not started |
| 7 | [ValueScenery](7-value-scenery.md) | Not started |
| 8 | [GPUI adapter](8-gpui-adapter.md) | Not started |
| 9 | [Composition primitives](9-composition.md) | Not started |
| 10 | [Decommission vantage-live](10-decommission-live.md) | Not started |

MVP is stages 1–3 — at that point a Lens can wrap any Vista with
caching, callbacks, and refresh. Stage 4 unlocks live events. Stages
5–7 add the reactive surface. Stage 8 wires the first UI consumer.
Stages 9–10 are cleanup and composition.

Each stage opens with a discussion phase mirroring the vista plans
convention. Confirm interfaces before writing code; later stages
inherit decisions from earlier ones.

## Conventions

- Each stage begins with a **discussion phase** — confirm interface and
  scope with the user before implementation. Questions deferred from
  earlier discussion are listed there.
- Each step has a checkbox; tick as you go.
- Each stage references items in `../../TODO.md`,
  `../../vantage-vista/plans/`, and the role READMEs it relates to.
- Tests use `Result<(), Box<dyn Error>>` (or `vantage_core::Result<()>`
  when no foreign error type is involved) so `?` replaces `.unwrap()`.
- Callbacks borrow `&Dio` and return `Pin<Box<dyn Future + 'a>>`. The
  HRTB pattern is documented in `ARCHITECTURE.md`; new callback shapes
  should follow it.

## Dependencies on vista

Diorama depends on a stable Vista surface. The relevant vista stages:

- **Vista stage 4** — driver factories produce Vistas with honest
  capability advertisement. Required for `make_dio` to receive
  meaningful capability flags.
- **Vista stage 5** — operator vocabulary for conditions. Required for
  Sceneries to push down filters where the driver supports them.
- **Vista stage 5b** — pagination, sort, search, aggregates surface on
  Vista. Required so Diorama's Sceneries can call these methods on the
  cache Vista and on the master Vista uniformly.

Diorama can land stage 1–3 against vista's current state (eq-conditions
+ list/get/insert/count). Stage 5+ of Diorama (Sceneries) need vista's
stage 5b to be at least partly done — specifically `set_pagination`,
`add_order`, `add_search`.

## External references this overhaul tracks against

- `../../vantage-vista/plans/0-overview.md` — Vista's roadmap; Diorama
  sits on top of it.
- `../../vantage-vista/plans/5b-query-controls.md` — paired stage;
  query controls land on Vista, Diorama provides the cache-side fallback.
- `../../vantage-vista/plans/9-decommission.md` — decommissioning of
  `vantage-live` happens here (stage 10) and is referenced from there.
- `../../TODO.md` "Wire up real LIVE query support end-to-end" —
  closed by stage 4 of this plan.
- `../../TODO.md` "AnyTable CBOR-swap follow-up" — orthogonal; vista
  stage 9 handles those entries.
