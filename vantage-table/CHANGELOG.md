# Changelog

## 0.6.0 — 2026-06-10

- `get_count_via_query` now unwraps single-element arrays (`[{"count": N}]`), matching how
  SQL/Surreal count queries commonly return results.
- Unrecognized result shapes are now surfaced as errors instead of silently returning 0.

## 0.5.7 — 2026-06-06

### Changed

- Docs only: reference-traversal documentation now points to
  [`vantage-vista-factory`](https://crates.io/crates/vantage-vista-factory)'s `VistaCatalog` for
  cross-persistence traversal. No functional or API change.

## 0.5.6 — 2026-06-02

### Added

- A table can now be sourced from an arbitrary sub-`SELECT`, not just a named table.
  [`Table::from_select(ds, alias, select)`](https://docs.rs/vantage-table/0.5.6/vantage_table/table/struct.Table.html)
  builds a derived table that renders `FROM (<select>) AS <alias>`.
- [`SelectSource<S>`](https://docs.rs/vantage-table/0.5.6/vantage_table/source/enum.SelectSource.html)
  — the source enum (`Name` or `Query { select, alias }`) that SQL/SurrealDB backends use for their
  new `TableSource::Source` associated type. Other backends keep `String`.

### Changed

- `TableSource` gains a required `type Source: TableSourceSpec`. Built-in backends are updated;
  out-of-tree `TableSource` impls must add the associated type (use `String` for a plain named
  source).

## 0.5.5 — 2026-05-31

### Added

- `Table::with_contained_one` / `with_contained_many` — declare an embedded object/array column as a
  contained relation, surfaced through Vista as an editable sub-table. The closure builds the
  contained record's schema, reusing the normal `Table` column machinery.
- `Table::get_contained_ref` — the generic, driver-agnostic resolution of a contained relation from
  a parent row (each backend supplies only its host-column encode/decode pair).
- `Table::with_contained_specs` — lowers a YAML `contained:` section into the same registrations,
  reusing the driver's existing column builder.
- `ContainedRelation<T>`, plus `Table::vista_contained()` / `contained_relation()` for driver
  factories. See the
  [contained relations guide](https://romaninsh.github.io/vantage/new-persistence/step9-contained-relations.html).

## 0.5.4 — 2026-05-30

Support for [vantage-vista 0.5.1](https://docs.rs/vantage-vista/0.5.1/vantage_vista/)'s nested
insert.

### Added

- `Table::get_ref_target::<E2>(relation)` — builds a relation's target table with **no** join
  condition (the bare table a new related row is inserted into), complementing the row-conditioned
  `get_ref_from_row` / `get_ref_as`.
- `Table::vista_references()` — snapshots the table's relations as `vantage_vista::Reference`s
  (name, target, cardinality, foreign key) for driver factories to fold into `VistaMetadata`.
- `Reference::foreign_key()` on the `Reference` trait — exposes the relation's FK column. **Note:**
  a new required trait method; external `Reference` impls (beyond the built-in `HasOne` / `HasMany`)
  must add it.

## 0.5.3 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.2 — 2026-05-23

The `AnyTable` type-erasing carrier is gone. Type erasure for cross-driver work now lives one layer
up in [`vantage_vista::Vista`](https://docs.rs/vantage-vista/0.4/vantage_vista/struct.Vista.html) —
wrap any typed `Table<T, E>` with `T::vista_factory().from_table(...)` to get a `Vista` carrying
`Record<ciborium::Value>` regardless of the underlying driver.

### Removed

- The `vantage_table::any` module — `AnyTable`, `AnyRecord`, `CborAdapter`, and the inline tests are
  all deleted.
- `Reference::resolve_as_any` — the AnyTable-returning trait method on `Reference`.
- `Table::get_ref` — the legacy method returning `AnyTable`. The typed `Table::get_ref_as` and
  `Table::get_subquery_as` survive; for the row-driven case prefer `Table::get_ref_from_row`.
- `TableLike::get_ref` — the AnyTable-returning default on the `TableLike` trait. The trait itself
  stays (it's used independently by `Box<dyn TableLike>` consumers).
- The commented-out `vantage_table::models_macro` and the `AnyTable`-only `with_pagination` test
  block.
- The `ref_example` example — the AnyTable-flavoured demo it covered is folded into
  `vantage-vista`'s mock-shell + driver factory examples.

### Carried over

- `Table::get_ref_as` and `Table::get_subquery_as` continue to work — their internals route through
  `Reference::build_target` and return typed `Table<T, E2>`, no `AnyTable` involvement.

## 0.5.1 — 2026-05-23

- Restored `tests/table_like.rs`. The previous AnyTable-on-`MockTableSource` tests were disabled
  during the CBOR swap; the file now runs as Vista smoke tests against
  [`MockShell`](https://docs.rs/vantage-vista/0.4/vantage_vista/mocks/struct.MockShell.html) — six
  tests covering table-name/column metadata, value round-trip via `ReadableValueSet` /
  `WritableValueSet` / `InsertableValueSet`, count, and `get_some_value`. These tests survive the
  AnyTable removal scheduled for the next release.
- `MockTableSource` stays JSON-typed for now. The original plan called for converting it to
  `ciborium::Value` so it could bridge into the (about-to-be-removed) `AnyTable`, but with
  `AnyTable` going away that's no longer needed — Vista-flavoured tests use
  [`vantage_vista::mocks::MockShell`](https://docs.rs/vantage-vista/0.4/vantage_vista/mocks/struct.MockShell.html)
  directly.

## 0.5.0 — 2026-05-23

Opens the 0.5 cycle. No code changes in this release beyond a docstring tidy; the version bump marks
the start of the `AnyTable` decommission — the `AnyTable` carrier, the legacy `Table::get_ref` /
`get_ref_as` / `get_subquery_as` methods, and `Reference::resolve_as_any` / `build_target` are
scheduled for removal across the 0.5.x line.

- Dropped a stale cross-link to `vantage_live::LiveTable` from the `AnyTable::from_table_like`
  docstring — the `vantage-live` crate has been removed from the workspace (superseded by
  `vantage-diorama`).

## 0.4.12 — 2026-05-17

- New
  [`Table::clear_orders`](https://docs.rs/vantage-table/0.4.12/vantage_table/struct.Table.html#method.clear_orders)
  drops every order clause — both permanent and temporary. Vista's
  [`add_order`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_order)
  is replace-semantics; this is the primitive its driver shells use to wipe state before pushing the
  new order.

## 0.4.11 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.10 — 2026-05-16

Row-based reference traversal lands at the typed layer; `HasForeign` retires.

- New
  [`Table::get_ref_from_row<E2>(relation, &row)`](https://docs.rs/vantage-table/0.4.10/vantage_table/struct.Table.html#method.get_ref_from_row)
  — reads the join field out of a known parent record and returns `Table<T, E2>` narrowed by one
  eq-condition. No subquery, no deferred fetch.
- New
  [`Table::with_id(id)`](https://docs.rs/vantage-table/0.4.10/vantage_table/struct.Table.html#method.with_id)
  convenience — pairs with `get_some_value` for the "I only know an id" workflow.
- New
  [`Reference::resolve_from_row`](https://docs.rs/vantage-table/0.4.10/vantage_table/references/trait.Reference.html#tymethod.resolve_from_row)
  and
  [`Reference::cardinality`](https://docs.rs/vantage-table/0.4.10/vantage_table/references/trait.Reference.html#tymethod.cardinality)
  trait methods. `HasOne` / `HasMany` implement both; the row-based path lives here and is called by
  the typed `get_ref_from_row` and by every Vista shell. `cardinality()` returns
  [`vantage_vista::ReferenceKind`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/enum.ReferenceKind.html)
  directly — drivers don't need a translation step. Adds `vantage-vista` as a direct dep (was
  already transitive via `vantage-expressions`).
- New
  [`TableSource::eq_value_condition(&self, field, value)`](https://docs.rs/vantage-table/0.4.10/vantage_table/traits/table_source/trait.TableSource.html#method.eq_value_condition)
  — typed-value sibling of `eq_condition`. Default errors; backends that participate in row-based
  traversal override.
- New `Table::ref_kinds() -> Vec<(String, ReferenceKind)>` and
  `Table::ref_cardinality(relation) -> Result<ReferenceKind>` for inspecting registered references
  with cardinality.
- **Breaking**: `HasForeign`, `Table::with_foreign`, `Table::is_foreign_ref`, and
  `Reference::is_foreign` are removed. Cross-persistence refs now live at the Vista layer
  ([`Vista::with_foreign`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/struct.Vista.html#method.with_foreign))
  — `vantage-table` stays single-backend by construction. The one in-tree caller (`vantage-aws`
  Lambda's `log_group`) migrated to a Vista-side registration; out-of-tree callers move their
  closure to `Vista::with_foreign` at the factory site.
- Legacy `Table::get_ref` / `get_ref_as` / `get_subquery_as` and `Reference::resolve_as_any` /
  `build_target` stay for now — slated for deletion in Stage 9 alongside `AnyTable`.

## 0.4.9 — 2026-05-15

- New
  [`TableLike::set_table_name(String)`](https://docs.rs/vantage-table/0.4.9/vantage_table/traits/table_like/trait.TableLike.html#method.set_table_name)
  trait method (default no-op) and matching inherent
  [`Table::set_table_name`](https://docs.rs/vantage-table/0.4.9/vantage_table/struct.Table.html#method.set_table_name).
  `AnyTable` forwards. Lets drivers that use the `table_name` field as a request-shape (REST API
  endpoints, URI templates) swap it at reference-traversal time without rebuilding the table.

## 0.4.8 — 2026-04-30

- `TableLike` gains five reflection / mutation methods so type-erased callers (the new model-driven
  CLI in `vantage-cli-util`, anything else holding an `AnyTable`) can reach metadata that previously
  only existed on the typed side. All have default impls so existing implementors compile unchanged.
  - `id_field_name() -> Option<String>`
  - `title_field_names() -> Vec<String>`
  - `column_types() -> IndexMap<String, &'static str>`
  - `get_ref_names() -> Vec<String>`
  - `add_condition_eq(field, value) -> Result<()>`
- New `TableSource::eq_condition(field, value) -> Result<Self::Condition>` (default: error).
  Backends that support textual eq filtering override; `add_condition_eq` dispatches through it.
- New `Table::with_title_column_of::<Type>(name)` builder — adds a typed column and records its name
  in an ordered `title_fields: Vec<String>` on the table. `title_field_names()` returns that vec,
  then unions in any columns that carry `ColumnFlag::TitleField` directly.
- `AnyTable` and the internal `CborAdapter` forward all five new methods through to the wrapped
  table.

## 0.4.7 — 2026-04-29

- New `AnyTable::get_ref(relation) -> Result<AnyTable>`. Lets reference traversal continue on the
  type-erased side — previously `get_ref` only existed on the typed `Table<T, E>`, so once you
  wrapped a table into `AnyTable` the relation graph was unreachable.
- `TableLike` gains a `get_ref` method with a default impl that errors out
  (`"get_ref not supported on this TableLike"`). `Table<T, E>` overrides to delegate to the
  inherent; `AnyTable` and the internal `CborAdapter` forward to the wrapped table; downstream
  wrappers like `vantage_live::LiveTable` override to forward through to their master.
- The trait method is sync — `Reference::resolve_as_any` is sync (no IO), so callers don't need an
  `.await`.

## 0.4.6 — 2026-04-26

- New `AnyTable::from_table_like<T: TableLike<…>>` constructor. Wraps any table-like type that
  already speaks `Value = CborValue, Id = String` — needed by `vantage-live::LiveTable`, which is
  `TableLike` but not a `Table<T, E>` instance.

## 0.4.5 — 2026-04-26

- New `ColumnFlag::Indexed` variant. UI hint that a column is cheap to sort or filter on;
  `vantage-redb` is the only backend that uses it for actual index maintenance.

## 0.4.4 — 2026-04-25

- **Breaking**: `AnyTable` now carries `ciborium::Value` instead of `serde_json::Value`. Backends
  already store CBOR (surreal/sql) or BSON (mongo) internally; this drops the last lossy JSON hop at
  the type-erased boundary.
- Renames the internal `JsonAdapter` to `CborAdapter`. Constructor signatures (`AnyTable::new` /
  `AnyTable::from_table`) unchanged at call sites.
- New `CborValueExt` trait re-exported from `prelude` — adds `as_str()`, `as_i64()`, `as_u64()`,
  `as_f64()`, `get(&str)`, `get_mut(&str)` so consumer code stays one-liner-shaped on top of
  `Record<ciborium::Value>`.
- Migration: AnyTable consumers that matched on JSON variants need to either match on
  `ciborium::Value` arms or call `serde_json::to_value(&record)?` at the consumer edge.
