# Stage 1 — vantage-vista crate skeleton

Status: **Done**

Set up the new crate with type definitions and trait surfaces only. No
execution logic. No driver implementations. Output: workspace compiles
and `vantage-vista` exposes the public API shape we'll build on.

## Discussion phase

Confirmed with the user:

- [x] `Vista` struct fields — name, columns (`IndexMap<String, Column>`),
      references (`IndexMap<String, Reference>`), capabilities, id_column,
      title_columns, eq_conditions (`Vec<(String, CborValue)>`), source
      (`Box<dyn VistaSource>`).
- [x] `VistaSource` trait method list — async `list`, `get`, `insert`,
      `replace`, `delete`, `count`; sync `capabilities`. No associated
      types, no `translate_condition`. `vantage_core::Result` everywhere.
- [x] `VistaFactory` shape — single trait with `from_yaml` only;
      driver-specific factories add their own inherent
      `from_table<E>(Table<DriverSource, E>)` to avoid a vantage-vista →
      vantage-table → vantage-expressions → vantage-vista cycle.
- [x] `VistaCapabilities` struct — explicit fields (`can_count`,
      `can_insert`, `can_update`, `can_delete`, `can_subscribe`,
      `can_invalidate`, `paginate_kind`).
- [x] What moves: `AnyExpression` + `ExpressionLike` move from
      `vantage-expressions` into `vantage-vista`; vantage-expressions
      keeps a re-export. `Column` and `Reference` are *new* parallel
      types in vantage-vista — `vantage_table::ColumnFlag` does **not**
      come along; driver factories translate flags into vista's purpose-
      bucketed accessors (`get_id_column`, `get_title_columns`).
- [x] Cargo features and workspace placement — no features; member of
      the root workspace.
- [x] Carrier types — methods take `&CborValue` for ids and
      `&Record<CborValue>` for records. `VistaSource` does **not**
      mirror `TableSource`; no `type Id` / `type Value` ceremony.

## Scope

In:

- New `vantage-vista` crate at workspace root
- `Vista` struct definition (no methods that touch the source yet)
- `VistaSource` trait skeleton (async CRUD, condition translation,
  capabilities)
- `VistaFactory` trait skeleton (`from_yaml`, `from_table`)
- `VistaCapabilities` struct
- `Column` definition (column metadata as data)
- `Reference` definition (reference metadata as data)
- `AnyExpression` migration in (currently in `vantage-expressions`)
- Error model (re-use `vantage_core::Result`)

Out:

- All implementations (drivers come in stage 2+)
- YAML parsing (stage 3)
- Hooks (stage 6)
- Coop layer (stage 7)
- Per-column condition policy (stage 5)

## Plan

- [x] Create `vantage-vista/` crate directory structure
- [x] Add to root `Cargo.toml` workspace members
- [x] `Cargo.toml`: deps `vantage-core`, `vantage-types`, `ciborium`,
      `indexmap`, `async-trait`, `serde`, `thiserror`
- [x] Define `Vista` struct (name, columns, references, capabilities,
      source — minimal fields)
- [x] Define `VistaSource` trait — `list`, `get`, `insert`, `replace`,
      `delete`, `count`, `capabilities` (no `translate_condition`; the
      Vista's eq-condition list is read directly by drivers)
- [x] Define `VistaFactory` trait — `from_yaml` only on the trait;
      `from_table` is a per-driver inherent method to avoid a
      vista → table → expressions → vista cycle
- [x] Define `VistaCapabilities` struct (explicit fields:
      `can_count`, `can_insert`, `can_update`, `can_delete`,
      `can_subscribe`, `can_invalidate`, `paginate_kind`)
- [x] Define `Column` struct (name, original_type, hidden — no
      `ColumnFlag`; driver factories bucket by purpose into vista
      accessors instead)
- [x] Define `Reference` struct (name, target, kind, foreign_key)
- [x] Move `AnyExpression` from `vantage-expressions` → `vantage-vista`;
      keep a re-export from `vantage-expressions` to preserve callers
- [x] `cargo check --workspace` passes
- [x] No unit tests yet; this is a shape-only stage

## References

- Subsumes (preparation only): PLAN_0_5 §1 (column visibility), §2
  (column serialisation), §3 (hooks)
- Touches: `../../TODO.md` "Refactor Expressions — split out Owned and
  Lazy expressions" — adjacent, may inform AnyExpression home
