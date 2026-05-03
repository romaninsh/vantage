# Changelog

## 0.4.0 — 2026-05-03

First release — incubating. New crate housing `Vista`, the universal,
schema-bearing data handle that drivers, scripting, UI, and agents will
consume in place of `AnyTable`. This stage is shape-only — no driver wiring,
no YAML loader, no hooks; the trait surface and metadata structs land first
so downstream stages can build against a stable API.

- `Vista` — concrete struct that owns universal metadata (name, columns,
  references, capabilities, current eq-conditions) plus a boxed `VistaSource`
  executor. Mutators: `add_column`, `add_reference`, `set_id_column`,
  `set_title_columns`, `add_condition_eq`. Purpose-bucketed accessors:
  `get_id_column`, `get_title_columns`, `get_column_names`, `get_column`,
  `get_references`, `get_reference`. Plus an inherent `get_count`.
- `VistaSource` — async trait drivers implement to back a `Vista`. Methods
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
- `MockVistaSource` — in-memory `IndexMap<String, Record<CborValue>>` source
  for tests. Filters `list_vista_values` by `Vista::eq_conditions`,
  auto-generates sequential string ids on `insert_vista_return_id_value`.
- 11 unit tests cover metadata accessors, the eq-condition list filter, and
  the full `WritableValueSet` / `InsertableValueSet` round-trip via the mock.
