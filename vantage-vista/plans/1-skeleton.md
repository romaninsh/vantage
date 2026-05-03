# Stage 1 — vantage-vista crate skeleton

Status: **Not started**

Set up the new crate with type definitions and trait surfaces only. No
execution logic. No driver implementations. Output: workspace compiles
and `vantage-vista` exposes the public API shape we'll build on.

## Discussion phase

Confirm with the user:

- [ ] `Vista` struct fields — what data does it own?
- [ ] `VistaSource` trait method list — async signatures, error type
- [ ] `VistaFactory` shape — `from_yaml` + `from_table` on one trait, or
      split?
- [ ] `VistaCapabilities` struct — explicit fields or open-ended map?
- [ ] What moves from `vantage-expressions` (`AnyExpression`) and
      `vantage-table` (column / reference metadata types) into
      `vantage-vista`; what stays
- [ ] Cargo features and workspace placement
- [ ] Carrier types: `Id = CborValue`, `Value = CborValue` confirmed at
      the `VistaSource` boundary

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

- [ ] Create `vantage-vista/` crate directory structure
- [ ] Add to root `Cargo.toml` workspace members
- [ ] `Cargo.toml`: deps `vantage-core`, `vantage-types`, `ciborium`,
      `indexmap`, `async-trait`, `serde`, `thiserror`
- [ ] Define `Vista` struct (name, columns, references, capabilities,
      source — minimal fields)
- [ ] Define `VistaSource` trait — `list`, `get`, `insert`, `replace`,
      `delete`, `count`, `capabilities`, `translate_condition`
- [ ] Define `VistaFactory` trait — `from_yaml`, `from_table`
- [ ] Define `VistaCapabilities` struct (explicit fields:
      `can_count`, `can_insert`, `can_update`, `can_delete`,
      `can_subscribe`, `can_invalidate`, `paginate_kind`)
- [ ] Define `Column` struct (name, type marker, flags, condition policy
      placeholder)
- [ ] Define `Reference` struct (name, target, kind, foreign_key)
- [ ] Move `AnyExpression` from `vantage-expressions` → `vantage-vista`;
      keep a re-export from `vantage-expressions` to preserve callers
- [ ] `cargo check --workspace` passes
- [ ] No unit tests yet; this is a shape-only stage

## References

- Subsumes (preparation only): PLAN_0_5 §1 (column visibility), §2
  (column serialisation), §3 (hooks)
- Touches: `../../TODO.md` "Refactor Expressions — split out Owned and
  Lazy expressions" — adjacent, may inform AnyExpression home
