# Changelog

## 0.6.5 — unreleased

### Added

- `MockShell` foreign-key traversal: `with_ref_target(relation, shell)` plus a
  `get_ref` that resolves a declared reference to another in-memory store,
  narrowed by the parent row's id. A `set_fail_reads` toggle simulates a source
  that goes offline. Together they let tests exercise reference traversal and
  failure-tolerance end to end (used by `vantage-diorama`'s `Dio::get_ref`).

### Changed

- `Vista::get_ref` docs no longer claim "same-persistence only": persistence is
  the shell's concern; callers stay persistence-agnostic.

## 0.6.3 — unreleased

### Added

- `MockShell` mutation helpers (`set_record`, `set_field`, `remove_record`, `clear_records`): opt-in,
  interior-mutable knobs for scripting server-side dataset changes mid-test. Back `vantage-diorama`'s
  scriptable test source (refresh / reload / soft-refresh scenarios). Read-only `MockShell` behaviour
  is unchanged.

## 0.6.2 — unreleased

### Added

- `augment_source_closure(resolver, code)` and `eval_augment_source(engine, code, base, row)` (rhai
  feature): build a reusable closure that narrows a pre-built `base` Vista in place from a master
  `row`, exposing the base as `self` and the row as `row`. Backs `vantage-diorama`'s scripted
  augmentation sources — all Rhai engine assembly stays here, so consumers only flip the `rhai`
  feature. Re-exported alongside the existing conventional/fetch vocabulary.

## 0.6.1 — unreleased

- Folded the read-only Rhai data-fetch surface (formerly the standalone `vantage-data-script` crate)
  into the `rhai` feature: terminal `list`/`get_some`/`count`/`capabilities`/`columns`/`references`
  verbs plus a `run_script` runner, alongside the existing builder vocabulary. The feature now pulls
  `serde_json` + `tokio` for the sync↔async bridge.
- Added random-access pagination: `VistaCapabilities::can_fetch_window`, `Vista::fetch_window`, and
  the `TableShell::fetch_window` default.

## 0.6.0 — unreleased

- Coordinated 0.6 release; internal dependencies realigned to 0.6. No public API changes.

## 0.5.5 — 2026-06-07

### Added

- `Vista::index_key(conditions, sort)` — a stable per-query identity string
  (name + normalized conditions + sort). Two-pass loading keys its ordered index
  by this, so filter/sort variants of the same entity get distinct indexes while
  sharing the name-keyed detail store.

## 0.5.4 — 2026-06-07

### Added

- Rhai-scripted reference traversal (escape hatch over the fixed foreign-key path). YAML
  stays the primary table-definition format; a per-reference Rhai script is a targeted,
  serializable override that builds the relationship's target `Vista` at traversal time.
  Gated behind the new optional `rhai` feature.
  - `Reference::build_script: Option<String>` plus `Reference::with_build_script(..)`, carrying
    the per-reference script lowered from the backend extras slot. Lazy: it only evaluates on
    traversal, so self/cyclic relations cost nothing until walked.
  - `VistaCapabilities::can_build_ref_via_script` advertises the capability, mirroring the
    `can_traverse_to_set` precedent.
  - `TableShell::register_rhai_extensions(&self, &mut rhai::Engine)` — default no-op hook
    backends override to contribute vendor vocabulary (expression syntax, `with_condition`).
  - New `rhai_conventional` module owns the engine with a *conventional, uniform* vocabulary
    over the type-erased `Vista` (`table`, `with_id`, `add_condition_eq`, `add_order`,
    `add_search`, `set_page_size`, `get_ref`), so any datasource — even engine-less ones —
    supports the script vocabulary and only loses vendor expression syntax (graceful
    degradation). Exposes `RhaiVista`, `TargetResolver`, `register_conventional_onto`,
    `eval_ref_script`, and `eval_modify_script` (the last applies a script to an
    already-built vista).

## 0.5.3 — 2026-06-06

### Removed

- **Breaking:** cross-persistence traversal is no longer a `Vista` concern. Removed
  `Vista::with_foreign`, the `ForeignResolver` / `ForeignRef` types, and the internal
  `foreign_resolvers` registry. A `Vista` is now strictly single-persistence: `get_ref`
  routes contained relations to the embedded sub-`Vista` and forwards everything else to
  the underlying shell. Resolving a reference that crosses persistence boundaries is now
  the job of [`vantage-vista-factory`](https://crates.io/crates/vantage-vista-factory)'s
  `VistaCatalog`.

## 0.5.2 — 2026-05-31

### Added

- Contained relations surface embedded objects/arrays as a full editable sub-`Vista` via
  `build_contained_vista`. Reads project the host column into records; writes re-serialize the
  whole collection and patch the parent row through a writeback closure. Contained records can
  traverse out to real tables, and a `contained:` section is now decodable from YAML specs. See the
  [contained relations guide](https://romaninsh.github.io/vantage/new-persistence/step9-contained-relations.html).

## 0.5.1 — 2026-05-30

### Added

- **Nested insert through relations.** `insert_value` / `insert_return_id_value` now accept a record
  whose keys name a **relation** instead of a column, and sequence the writes so foreign keys are
  populated automatically: a **has-one** child (`inventory` as a map, or grouped `inventory.count` /
  `inventory.flag` keys) is inserted first and its id stamped into the parent's FK column; **has-many**
  children (`orders` as a list of maps) are inserted after the parent with the parent's id stamped
  into each child's FK column. Arbitrary depth, native (same-persistence) relations only. Best-effort
  and non-atomic — a mid-sequence failure leaves earlier writes committed. Vista does no field
  validation; the underlying table validates each record. Cross-persistence
  ([`with_foreign`](https://docs.rs/vantage-vista/0.5.1/vantage_vista/struct.Vista.html#method.with_foreign))
  relations are rejected.
- [`TableShell::get_ref_target`](https://docs.rs/vantage-vista/0.5.1/vantage_vista/trait.TableShell.html)
  and `Vista::get_ref_target` — build the **bare** (unconditioned) target of a relation, i.e. the
  table a new related row is inserted into. Default impl returns `Unimplemented`.

### Changed

- Driver factories now populate `VistaMetadata::references`, so `Vista::get_reference` carries the
  relation's `foreign_key` for code-first vistas (previously empty). Requires
  [vantage-table 0.5.4](https://docs.rs/vantage-table/0.5.4/vantage_table/).

## 0.5.0 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.4.12 — 2026-05-23

- Doc comment touch-up on
  [`TableShell`](https://docs.rs/vantage-vista/0.4.12/vantage_vista/trait.TableShell.html) — drops
  the `AnyTable` reference now that
  [vantage-table 0.5.2](https://docs.rs/vantage-table/0.5.2/vantage_table/) removes it. No code
  change.

## 0.4.11 — 2026-05-23

- Dep-pin bump on `vantage-dataset` to 0.5; tracks the `ImTable` / `ImDataSource` parametrization.
  No public API change.

## 0.4.10 — 2026-05-18

- **Breaking**: schema moves to the shell.
  [`TableShell`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/trait.TableShell.html) gains
  three required methods —
  [`columns`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/trait.TableShell.html#tymethod.columns),
  [`references`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/trait.TableShell.html#tymethod.references),
  [`id_column`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/trait.TableShell.html#tymethod.id_column)
  — and
  [`Vista::new(name, source)`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/struct.Vista.html#method.new)
  loses its `VistaMetadata` parameter. Driver shells store the metadata themselves and answer schema
  queries directly; `Vista` forwards every metadata accessor to the shell, and
  `Vista::get_ref_kinds` derives from `shell.references()` by default. Driver crates that wrap a
  typed `Table` are already updated.

## 0.4.9 — 2026-05-17

Stage 5 query primitives arrive at the universal surface — sort, quicksearch, and pagination on
every Vista.

- New
  [`Vista::add_order(column, dir)`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_order)
  and
  [`Vista::clear_orders`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.clear_orders)
  push a single ORDER BY clause down to the driver. Replace-semantics: calling `add_order` again
  wipes the previous one. V1 is single-column; the signature stays for when multi-column lands.
  Columns must carry the new
  [`ORDERABLE`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/flags/constant.ORDERABLE.html)
  flag; otherwise the call surfaces `Unsupported` (DynamoDB-style sort-key-only backends use this).
- New
  [`Vista::add_search(text)`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_search)
  and
  [`Vista::clear_search`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.clear_search)
  — one string fans out across the columns the driver considers searchable (typically those flagged
  [`SEARCHABLE`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/flags/constant.SEARCHABLE.html)).
  Also replace-semantics.
- New pagination triple:
  [`Vista::set_page_size(n)`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.set_page_size),
  [`Vista::fetch_page(page)`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.fetch_page)
  (offset-style, 1-based), and
  [`Vista::fetch_next(token)`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.fetch_next)
  (cursor-style; opaque driver-private token). Drivers advertise which they support — DynamoDB and
  most token-paginated REST APIs only get `fetch_next`; SQL gets all three.
- New [`SortDirection`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/enum.SortDirection.html)
  at the Vista boundary, mirroring `vantage-table`'s `SortDirection` without depending on it.
- **Breaking**:
  [`VistaCapabilities`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.VistaCapabilities.html)
  loses `paginate_kind: PaginateKind` and gains five flat booleans: `can_order`, `can_search`,
  `can_set_page_size`, `can_fetch_page`, `can_fetch_next`. The old enum collapsed the matrix; the
  new shape lets a driver advertise "yes I paginate but you can't pick the page size" or "yes
  random-access pages and forward cursor". The `PaginateKind` enum is removed from the public
  re-exports.
- Matching `TableShell` trait methods (`set_page_size`, `fetch_page`, `fetch_next`, `add_search`,
  `clear_search`, `add_order`, `clear_orders`) — all default to the capability-honest error pair
  (`Unimplemented` when the flag is `true`, `Unsupported` when `false`).
- `TableShell::default_error` no longer takes the `&Vista` parameter; drivers that override methods
  to fall back to it should drop the argument at the call site.
- [`MockShell`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/mocks/struct.MockShell.html)
  implements `add_order` / `clear_orders` / `add_search` / `clear_search` end-to-end (single-column
  sort, case-insensitive substring search across text fields) and defaults `can_order` /
  `can_search` to `true`, so consumers can exercise the new vocabulary against the mock without
  standing up a real driver.

## 0.4.8 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.7 — 2026-05-16

Row-based reference traversal arrives at the universal surface, replacing the AnyTable-bridged path.

- **Breaking**:
  [`Vista::get_ref`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/struct.Vista.html#method.get_ref)
  signature is now `(relation, &Record<CborValue>)` — pass in the parent row, get back a child Vista
  narrowed by one eq-condition. Drops the subquery-based path and the deferred-fetch dance.
  [`TableShell::get_ref`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/trait.TableShell.html#method.get_ref)
  gets the matching signature; the unused `vista: &Vista` parameter was dropped — shells holding a
  typed `Table` don't consult Vista metadata to traverse.
- New
  [`Vista::with_foreign(name, kind, closure)`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/struct.Vista.html#method.with_foreign)
  — cross-persistence reference declaration at the Vista layer. The closure is _stored, never
  invoked_ at registration so mutually-referencing Vistas don't recurse at construction; it fires
  lazily on `get_ref`. `kind: ReferenceKind` records cardinality so consumers can render record-card
  vs list-grid.
- New
  [`Vista::with_id(id)`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/struct.Vista.html#method.with_id)
  convenience — narrow by id, pair with `get_some_value` for the "I only know an id" workflow.
- New
  [`Vista::list_references()`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/struct.Vista.html#method.list_references)
  returns `Vec<(name, ReferenceKind)>`. Combines foreign resolvers, YAML metadata, and the wrapped
  table's typed refs surfaced via the new
  [`TableShell::get_ref_kinds`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/trait.TableShell.html#method.get_ref_kinds).
- **Breaking**: `ReferenceKind::HasForeign` is removed — the enum is `HasOne | HasMany`.
  Cross-persistence-ness is no longer encoded in the kind; it's determined at resolution time by
  whether the target Vista lives in the same driver or a different one (the inventory loader knows).
  YAML files using `kind: has_foreign` migrate to `kind: has_one` or `kind: has_many` depending on
  cardinality.
- Step 8 of the Vista integration guide gets a new "References delegate too" section plus an
  Optional-overrides walkthrough — `docs4/src/new-persistence/step8-vista-integration.md`.

## 0.4.6 — 2026-05-15

- New
  [`Vista::add_raw_condition<C>(condition: C)`](https://docs.rs/vantage-vista/0.4.6/vantage_vista/struct.Vista.html#method.add_raw_condition)
  and matching
  [`TableShell::add_raw_condition`](https://docs.rs/vantage-vista/0.4.6/vantage_vista/trait.TableShell.html#method.add_raw_condition)
  trait method (default returns `Unimplemented`). Drivers can downcast the boxed value to their own
  `T::Condition` and push it directly into the wrapped table. Used by YAML factories that need to
  inject deferred-FK eq conditions (where the value is read out of a parent record at fetch time),
  which the scalar `add_condition_eq` channel can't express.

## 0.4.5 — 2026-05-14

- New
  [`Vista::get_ref`](https://docs.rs/vantage-vista/0.4.5/vantage_vista/struct.Vista.html#method.get_ref)
  traverses a named reference and returns the related `Vista`. The driver does the work — it
  consults its wrapped typed table's `with_one` / `with_many` registry, applies the join condition,
  and wraps the result back into a `Vista` so callers stay on the universal surface.
- New
  [`TableShell::get_ref`](https://docs.rs/vantage-vista/0.4.5/vantage_vista/trait.TableShell.html#method.get_ref)
  trait method (default returns `Unimplemented`) — drivers wrapping a typed `Table<T, E>` can
  delegate to `Table::get_ref` and the rest is automatic. The first driver opting in is
  [`vantage-api-client 0.1.4`](https://docs.rs/vantage-api-client/0.1.4/).

## 0.4.4 — 2026-05-04

- New
  [`Vista::driver()`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/struct.Vista.html#method.driver)
  returns a short label for the backing driver (`"csv"`, `"sqlite"`, `"postgres"`, `"mysql"`,
  `"mongodb"`) — handy for diagnostics and CLI output.
- New
  [`TableShell::driver_name`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/trait.TableShell.html#method.driver_name)
  trait method (default `"unknown"`) drives the above; in-tree drivers all override.

## 0.4.3 — 2026-05-04

Renames the driver-facing trait so its name describes what it actually is.

- The trait formerly known as `VistaSource` is now
  [`TableShell`](https://docs.rs/vantage-vista/0.4.3/vantage_vista/trait.TableShell.html). It wraps
  a typed `Table<T, E>` and exposes it through the CBOR/`String` boundary — "shell" reads more
  accurately than "source", which already meant something else in `TableSource`.
- The in-tree mock follows: `MockVistaSource` →
  [`MockShell`](https://docs.rs/vantage-vista/0.4.3/vantage_vista/mocks/struct.MockShell.html).
- **Breaking** for anyone naming the trait directly. Existing in-tree drivers (`vantage-csv`,
  `vantage-mongodb`) move with the rename in lock-step; downstream drivers need a one-line change at
  their `impl` site and at every `Box<dyn VistaSource>`.
- New driver-author guide at
  [Step 8: Vista Integration](https://romaninsh.github.io/vantage/new-persistence/step8-vista-integration.html)
  — distilled from the CSV and MongoDB rollouts, covering the factory split, the `column_paths`
  pattern for nested fields, the capability honesty contract, and the tests that catch the common
  mistakes.

## 0.4.2 — 2026-05-04

Conditions now delegate to the driver instead of being stashed on `Vista`.

- [`Vista::add_condition_eq`](https://docs.rs/vantage-vista/0.4.2/vantage_vista/struct.Vista.html#method.add_condition_eq)
  returns `Result<()>` and forwards to the source. The internal `eq_conditions` vec is gone — Vista
  carries no condition state.
- New
  [`TableShell::add_eq_condition`](https://docs.rs/vantage-vista/0.4.2/vantage_vista/trait.TableShell.html#method.add_eq_condition)
  trait method (default impl returns `Unimplemented`). Drivers translate `(field, CborValue)` into
  their native condition type and push it onto the wrapped table — server-side push-down is
  automatic wherever the backend supports it.
- **Breaking** for in-tree `TableShell` implementors: `add_condition_eq` now returns `Result`, so
  callers need `?` (or `.unwrap()`) at every call site. `Vista::eq_conditions()` accessor removed.
- Requires `vantage-core 0.4.1` for the new `is_unimplemented` / `is_unsupported` annotators on
  default-impl errors.

## 0.4.1 — 2026-05-03

Stage 3 — universal YAML loader.

- New
  [`VistaSpec<T, C, R>`](https://docs.rs/vantage-vista/0.4.1/vantage_vista/struct.VistaSpec.html) is
  the YAML-facing schema. Three generic parameters carry driver-specific extras at the table,
  column, and reference level (use
  [`NoExtras`](https://docs.rs/vantage-vista/0.4.1/vantage_vista/struct.NoExtras.html) when a driver
  has none). Sugar form `references: products` parses as a
  [`ReferenceSugar::Sugar`](https://docs.rs/vantage-vista/0.4.1/vantage_vista/enum.ReferenceSugar.html)
  and the full form deserialises a `ReferenceSpec`.
- [`VistaFactory`](https://docs.rs/vantage-vista/0.4.1/vantage_vista/trait.VistaFactory.html) gains
  three associated `Extras` types and a new `build_from_spec` method. The default `from_yaml` parses
  with `serde_yaml_ng` and dispatches — drivers only need to implement `build_from_spec`.
- New [`flags`](https://docs.rs/vantage-vista/0.4.1/vantage_vista/flags/index.html) module: `ID`,
  `TITLE`, `SEARCHABLE`, `MANDATORY`, `HIDDEN`. The vocabulary is open `Vec<String>`; these
  constants name the values vista's own accessors understand.
- **Breaking**: `Column::hidden: bool` is replaced by `Column::flags: Vec<String>`. Use
  `Column::with_flag`, `is_hidden`, `is_id`, `is_title` instead. `VistaMetadata::with_title_columns`
  is gone — title columns are derived from the `title` flag at runtime via
  `Vista::get_title_columns()`. Driver factories that translated `ColumnFlag::Hidden` need to call
  `with_flag(flags::HIDDEN)` instead of `Column::hidden()`.

## 0.4.0 — 2026-05-03

First release — incubating. New crate housing `Vista`, the universal, schema-bearing data handle
that drivers, scripting, UI, and agents will consume in place of `AnyTable`. This stage is
shape-only — no driver wiring, no YAML loader, no hooks; the trait surface and metadata structs land
first so downstream stages can build against a stable API.

- `Vista` — concrete struct that owns universal metadata (name, columns, references, capabilities,
  current eq-conditions) plus a boxed `TableShell` executor. Mutators: `add_column`,
  `add_reference`, `set_id_column`, `set_title_columns`, `add_condition_eq`. Purpose-bucketed
  accessors: `get_id_column`, `get_title_columns`, `get_column_names`, `get_column`,
  `get_references`, `get_reference`. Plus an inherent `get_count`.
- `TableShell` — async trait drivers implement to back a `Vista`. Methods named with the `_vista_`
  infix (`list_vista_values`, `get_vista_value`, `get_vista_some_value`, `stream_vista_values`
  (default impl wrapping `list`), `insert_vista_value`, `replace_vista_value`, `patch_vista_value`,
  `delete_vista_value`, `delete_vista_all_values`, `insert_vista_return_id_value`,
  `get_vista_count`, `capabilities`) — mirrors the `_table_` convention on `TableSource` so the
  delegation pattern is identical.
- `Vista` impls `vantage_dataset::ValueSet` / `ReadableValueSet` / `WritableValueSet` /
  `InsertableValueSet`. `Id = String, Value = ciborium::Value` matches `AnyTable`'s existing pragma
  — `IndexMap<Self::Id, …>` needs `Hash + Eq` and `ciborium::Value` has neither, so backend-native
  ids (Mongo `ObjectId`, Surreal `Thing`) stringify at the source boundary.
- `VistaCapabilities` — explicit struct with named fields (`can_count`, `can_insert`, `can_update`,
  `can_delete`, `can_subscribe`, `can_invalidate`, `paginate_kind`). UI code branches on these
  instead of probing methods.
- `Column` — vista-side display metadata only (`name`, `original_type`, `hidden`).
  `vantage_table::ColumnFlag` does not come along — driver factories translate flags into Vista's
  purpose accessors during construction.
- `Reference` + `ReferenceKind` (`HasOne` / `HasMany` / `HasForeign`) — metadata-only relation
  descriptors.
- `VistaFactory` trait — single method `from_yaml(&str) -> Result<Vista>` for the universal loader
  (stage 3). `from_table<E>(Table<DriverSource, E>)` is intentionally an inherent method on each
  driver's concrete factory rather than a trait method, to avoid a
  `vantage-vista → vantage-table → vantage-expressions → vantage-vista` dependency cycle.
- `AnyExpression` + `ExpressionLike` move here from `vantage-expressions`, which now `pub use`s
  them. The seven existing call sites (`vantage-table` x3, `vantage-live` x2, `vantage-ui` adapter,
  the `vantage-expressions` prelude) compile unchanged.
- `MockShell` — in-memory `IndexMap<String, Record<CborValue>>` source for tests. Filters
  `list_vista_values` by `Vista::eq_conditions`, auto-generates sequential string ids on
  `insert_vista_return_id_value`.
- 11 unit tests cover metadata accessors, the eq-condition list filter, and the full
  `WritableValueSet` / `InsertableValueSet` round-trip via the mock.
