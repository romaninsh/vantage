# Changelog

## 0.6.12 — 2026-07-21

- `Table::with_active_columns(&["id", "client.name", "client.bakery.name"])` —
  **implicit references**. A plain name restricts projection to that column; a
  dotted name traverses declared `has_one` relations and imports the target's
  field as a read-only column aliased under the literal dotted name. Recursion
  is supported (`a.b.c`). Everything is validated when the table is built:
  unknown column/relation, a `has_many` hop, or a backend without traversal
  support are all build-time errors, never fetch-time surprises. SQL backends
  lower each hop into a nested correlated scalar subquery; a backend may
  override the new `TableSource::traversal_path_expr` hook to emit a native path
  instead (SurrealDB does). Imported columns are stripped from full-record
  write payloads (insert, replace, and the generated-id insert path) so a
  read-modify-save round-trip never persists them; a `patch` naming an
  imported column is rejected with an explicit read-only error instead of
  silently no-opping.
- New `TableSource::supports_traversal` / `traversal_path_expr` hooks (default
  `false` / `None`); `Table::select_expression` extracted from `select_column`
  so a traversal expression can nest. `traversal_path_expr` receives each
  hop's foreign-key/link *field* (not the relation's registry name), so a
  relation named differently from its link column still lowers correctly.
- Expression-only columns (registered via `with_expression` with no column
  def) can be named in the active set; `Table::is_imported_column` /
  `is_calculated_column` accessors let driver factories flag computed columns
  `calculated` in vista metadata.

## 0.6.11 — 2026-07-20

- `Table::apply_lazy_expressions` is public — a driver shell that bypasses
  the `list_values` read path (e.g. a REST shell's windowed fetch) can now
  apply `lazy:` columns to the rows it fetches directly.

## 0.6.10 — 2026-07-15

- `Table::with_lazy_expression(name, callback)` — a column computed in Rust on
  each *returned* record, after the source answers. Callbacks apply in
  declaration order and each borrows the record as built so far, so one
  expensive fetch (a file's contents) can feed several cheap derived columns
  declared after it. Lazy columns register in the table's schema and apply on
  the list/get/stream read paths (raw-record and entity forms), but are never
  projected into source queries — no conditions, ordering, or pushdown.

## 0.6.9 — 2026-06-28

- `Table::with_text_id()` marks the id column as a text/string key so SQL backends do not
  numerically coerce it. Without it the Postgres backend binds an all-digit id like `"121"` as
  `bigint`, which fails against a `TEXT` id column; the flag keeps such ids bound as text. Defaults
  off, so the integer-id convention other models rely on is unchanged. Read back via
  `Table::id_is_text()`.

## 0.6.8 — 2026-06-26

- `Table::with_generated_id(IdGenerator)` mints a record's id on insert when the backend does not
  (a bare SQL `PRIMARY KEY` with no `DEFAULT`, a client-keyed REST resource). `IdGenerator` offers
  `UuidV7` (time-ordered, index-friendly — the recommended default), `UuidV4`, and `Custom` for any
  other scheme. Built on a `before_insert` hook: it fills the id only when the record carries none
  (absent or null) and only on insert — `patch`/`replace`/`update`/`upsert` never touch the id, and
  a caller-supplied id is always kept, so generation stays idempotent.
- `Table::with_timestamps()` / `with_audit(Timestamps)` stamp audit columns from the wall clock:
  `created_at` once on insert (only if the caller left it empty), `updated_at` on every write. Values
  are RFC 3339 UTC strings; column names are overridable via `Timestamps`. Built on the same
  before-write hooks, so a nullable `TEXT` column is all the backend needs.

## 0.6.7 — 2026-06-25

- Lifecycle hooks on `Table`, registered with `Table::with_hook(Hook::…)`. The `Hook` enum carries
  a placement-specific async closure: `BeforeInsert`/`BeforeUpdate`/`BeforeSave` (and `After*`) run
  on the record around a write; `BeforeDelete`/`AfterDelete` run around a delete. Before-write hooks
  run ahead of invariant enforcement, ordered by `Phase` (`Normalize` → `Populate` → `Validate`,
  then registration order), and may mutate the record (audit stamps, normalization) or return an
  error to cancel the write. `BeforeDelete` may instead return `HookReturn::Handled` to take over —
  e.g. a soft-delete that patches a marker and skips the real `DELETE`. After-commit hooks fire for
  side-effects (the delete hook receives the row's former contents). Hooks receive the
  entity-erased table for relation/datasource access. Both the typed-entity and raw-record write
  paths fire them; `delete_all` does not.

## 0.6.6 — 2026-06-25

- `ActiveEntity::get_ref::<E2>("rel")` and `ActiveRecord::get_ref::<E2>("rel")` (via the new
  `GetRefExt` trait): traverse a relation from a loaded record, the record-level equivalent of
  `Table::get_ref_from_row`. For the typed `ActiveEntity` the entity's id is injected into the row
  before traversal so has-many relations resolve; the untyped `ActiveRecord` already holds the raw
  row and forwards directly.
- An equality scope is a set **invariant**: a table narrowed by a literal `column = value` (via
  `with_id` or `Reference::resolve_from_row`) carries that value as an invariant, so every row
  written into the set conforms to it (e.g. a has-many child carries its parent's foreign key). Only
  plain `column = value` scopes register an invariant — expression conditions do not. Enforcement is
  generic across all backends on insert/replace/patch: a column left null/absent is filled, a
  matching value is kept, and a conflicting value is rejected with an error. `Table::add_invariant`
  / `with_invariant` / `invariants` register and read them; the `InvariantValue` value trait
  supplies the null check and equality each backend needs.

## 0.6.5 — 2026-06-24

- Docs: `ExpressionFn`'s doc comment no longer intra-doc-links to the private `Table::as_entity_erased`,
  which broke `cargo doc -D warnings` (and docs.rs) under `rustdoc::private_intra_doc_links`. No API change.

## 0.6.4 — 2026-06-21

- `Pagination::window(offset, limit)` for random-access `[offset, offset + limit)` windows whose
  offset need not be a multiple of the page size (`skip()` returns the offset verbatim). Backs
  `Vista::fetch_window` on offset/limit-addressable drivers.
- `with_expression` computed columns now survive `into_entity` (and therefore reference traversal
  that erases the entity to `EmptyEntity`, such as `get_ref_from_row`). Previously `into_entity`
  dropped all expressions, so computed aggregates were present on a top-level table but silently
  missing from its nested/drilldown rows. `ExpressionFn` is now stored entity-erased; `with_expression`
  adapts the caller's `Fn(&Table<T, E>)` into it. Fixes the `get_ref_from_row` doc claim that
  expressions are preserved.

## 0.6.3 — 2026-06-18

- Added `ColumnFlag::Label` — hints a column is better shown as a small status tag attached to the
  record's title than as its own column (e.g. a status/state field with a per-value color map).

## 0.6.2 — 2026-06-17

- Internal dependency realignment for the coordinated 0.6 release; no public API changes.

## 0.6.1 — 2026-06-13

- Documented the contract for `TableSource::search_table_condition`: search is a server-side
  capability, and backends that cannot search must surface an `Unsupported` error when the
  condition resolves — never a silent match-all, never a panic.

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
