# Changelog

## 0.4.4 — 2026-05-04

- New [`Vista::driver()`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/struct.Vista.html#method.driver) returns a short label for the backing driver (`"csv"`, `"sqlite"`, `"postgres"`, `"mysql"`, `"mongodb"`) — handy for diagnostics and CLI output.
- New [`TableShell::driver_name`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/trait.TableShell.html#method.driver_name) trait method (default `"unknown"`) drives the above; in-tree drivers all override.

## 0.4.3 — 2026-05-04

Renames the driver-facing trait so its name describes what it actually is.

- The trait formerly known as `VistaSource` is now [`TableShell`](https://docs.rs/vantage-vista/0.4.3/vantage_vista/trait.TableShell.html). It wraps a typed `Table<T, E>` and exposes it through the CBOR/`String` boundary — "shell" reads more accurately than "source", which already meant something else in `TableSource`.
- The in-tree mock follows: `MockVistaSource` → [`MockShell`](https://docs.rs/vantage-vista/0.4.3/vantage_vista/mocks/struct.MockShell.html).
- **Breaking** for anyone naming the trait directly. Existing in-tree drivers (`vantage-csv`, `vantage-mongodb`) move with the rename in lock-step; downstream drivers need a one-line change at their `impl` site and at every `Box<dyn VistaSource>`.
- New driver-author guide at [Step 8: Vista Integration](https://romaninsh.github.io/vantage/new-persistence/step8-vista-integration.html) — distilled from the CSV and MongoDB rollouts, covering the factory split, the `column_paths` pattern for nested fields, the capability honesty contract, and the tests that catch the common mistakes.

## 0.4.2 — 2026-05-04

Conditions now delegate to the driver instead of being stashed on `Vista`.

- [`Vista::add_condition_eq`](https://docs.rs/vantage-vista/0.4.2/vantage_vista/struct.Vista.html#method.add_condition_eq) returns `Result<()>` and forwards to the source. The internal `eq_conditions` vec is gone — Vista carries no condition state.
- New [`TableShell::add_eq_condition`](https://docs.rs/vantage-vista/0.4.2/vantage_vista/trait.TableShell.html#method.add_eq_condition) trait method (default impl returns `Unimplemented`). Drivers translate `(field, CborValue)` into their native condition type and push it onto the wrapped table — server-side push-down is automatic wherever the backend supports it.
- **Breaking** for in-tree `TableShell` implementors: `add_condition_eq` now returns `Result`, so callers need `?` (or `.unwrap()`) at every call site. `Vista::eq_conditions()` accessor removed.
- Requires `vantage-core 0.4.1` for the new `is_unimplemented` / `is_unsupported` annotators on default-impl errors.

## 0.4.1 — 2026-05-03

Stage 3 — universal YAML loader.

- New [`VistaSpec<T, C, R>`](https://docs.rs/vantage-vista/0.4.1/vantage_vista/struct.VistaSpec.html) is the YAML-facing schema. Three generic parameters carry driver-specific extras at the table, column, and reference level (use [`NoExtras`](https://docs.rs/vantage-vista/0.4.1/vantage_vista/struct.NoExtras.html) when a driver has none). Sugar form `references: products` parses as a [`ReferenceSugar::Sugar`](https://docs.rs/vantage-vista/0.4.1/vantage_vista/enum.ReferenceSugar.html) and the full form deserialises a `ReferenceSpec`.
- [`VistaFactory`](https://docs.rs/vantage-vista/0.4.1/vantage_vista/trait.VistaFactory.html) gains three associated `Extras` types and a new `build_from_spec` method. The default `from_yaml` parses with `serde_yaml_ng` and dispatches — drivers only need to implement `build_from_spec`.
- New [`flags`](https://docs.rs/vantage-vista/0.4.1/vantage_vista/flags/index.html) module: `ID`, `TITLE`, `SEARCHABLE`, `MANDATORY`, `HIDDEN`. The vocabulary is open `Vec<String>`; these constants name the values vista's own accessors understand.
- **Breaking**: `Column::hidden: bool` is replaced by `Column::flags: Vec<String>`. Use `Column::with_flag`, `is_hidden`, `is_id`, `is_title` instead. `VistaMetadata::with_title_columns` is gone — title columns are derived from the `title` flag at runtime via `Vista::get_title_columns()`. Driver factories that translated `ColumnFlag::Hidden` need to call `with_flag(flags::HIDDEN)` instead of `Column::hidden()`.

## 0.4.0 — 2026-05-03

First release — incubating. New crate housing `Vista`, the universal,
schema-bearing data handle that drivers, scripting, UI, and agents will
consume in place of `AnyTable`. This stage is shape-only — no driver wiring,
no YAML loader, no hooks; the trait surface and metadata structs land first
so downstream stages can build against a stable API.

- `Vista` — concrete struct that owns universal metadata (name, columns,
  references, capabilities, current eq-conditions) plus a boxed `TableShell`
  executor. Mutators: `add_column`, `add_reference`, `set_id_column`,
  `set_title_columns`, `add_condition_eq`. Purpose-bucketed accessors:
  `get_id_column`, `get_title_columns`, `get_column_names`, `get_column`,
  `get_references`, `get_reference`. Plus an inherent `get_count`.
- `TableShell` — async trait drivers implement to back a `Vista`. Methods
  named with the `_vista_` infix (`list_vista_values`, `get_vista_value`,
  `get_vista_some_value`, `stream_vista_values` (default impl wrapping
  `list`), `insert_vista_value`, `replace_vista_value`, `patch_vista_value`,
  `delete_vista_value`, `delete_vista_all_values`, `insert_vista_return_id_value`,
  `get_vista_count`, `capabilities`) — mirrors the `_table_` convention on
  `TableSource` so the delegation pattern is identical.
- `Vista` impls `vantage_dataset::ValueSet` / `ReadableValueSet` /
  `WritableValueSet` / `InsertableValueSet`. `Id = String, Value =
  ciborium::Value` matches `AnyTable`'s existing pragma — `IndexMap<Self::Id, …>`
  needs `Hash + Eq` and `ciborium::Value` has neither, so backend-native ids
  (Mongo `ObjectId`, Surreal `Thing`) stringify at the source boundary.
- `VistaCapabilities` — explicit struct with named fields (`can_count`,
  `can_insert`, `can_update`, `can_delete`, `can_subscribe`, `can_invalidate`,
  `paginate_kind`). UI code branches on these instead of probing methods.
- `Column` — vista-side display metadata only (`name`, `original_type`,
  `hidden`). `vantage_table::ColumnFlag` does not come along — driver
  factories translate flags into Vista's purpose accessors during
  construction.
- `Reference` + `ReferenceKind` (`HasOne` / `HasMany` / `HasForeign`) —
  metadata-only relation descriptors.
- `VistaFactory` trait — single method `from_yaml(&str) -> Result<Vista>` for
  the universal loader (stage 3). `from_table<E>(Table<DriverSource, E>)` is
  intentionally an inherent method on each driver's concrete factory rather
  than a trait method, to avoid a `vantage-vista → vantage-table →
  vantage-expressions → vantage-vista` dependency cycle.
- `AnyExpression` + `ExpressionLike` move here from `vantage-expressions`,
  which now `pub use`s them. The seven existing call sites
  (`vantage-table` x3, `vantage-live` x2, `vantage-ui` adapter, the
  `vantage-expressions` prelude) compile unchanged.
- `MockShell` — in-memory `IndexMap<String, Record<CborValue>>` source
  for tests. Filters `list_vista_values` by `Vista::eq_conditions`,
  auto-generates sequential string ids on `insert_vista_return_id_value`.
- 11 unit tests cover metadata accessors, the eq-condition list filter, and
  the full `WritableValueSet` / `InsertableValueSet` round-trip via the mock.
