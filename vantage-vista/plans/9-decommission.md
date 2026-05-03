# Stage 9 — Decommission old types

Status: **Not started**

Once Vista is functionally complete and vantage-ui has migrated, remove
the older type-erased wrapper, the live-table types, and related shims.
Final cleanup pass; closes a long tail of TODO items deferred during the
0.4 swap.

## Discussion phase

- [ ] Confirm Vista feature parity — every use case the old wrapper
      supported has a Vista equivalent (with one explicit list of
      exceptions documented if any)
- [ ] Confirm vantage-ui has fully migrated and there are no external
      consumers of the old types
- [ ] Deprecation timing — single-cut at 0.5 release vs deprecate-and-warn
      for one minor cycle?
- [ ] `vantage-live` crate fate — fully removed (LiveStream/Cache moved
      to vantage-coop in stage 7), or kept as a thin re-export shim for
      one cycle?
- [ ] `bakery_model4` is currently excluded from the workspace; bring it
      into Vista or leave excluded?

## Scope

In:

- Delete `vantage-table/src/any.rs` (the old type-erased wrapper)
- Delete the old `TableLike` trait family (or whatever trait the wrapper
  boxed) if no other consumer remains
- Delete or shrink `vantage-live` (logic moved to vantage-coop)
- Delete the legacy `AnyTable` trait at `vantage/src/sql/table.rs`
  (unrelated to the new struct, dead-ish today)
- Restore disabled tests under their new home
- Update `bakery_model3` and `bakery_model4` to Vista
- Sweep examples (egui/tauri/tui/gpui/python) for old-type references

Out:

- Re-architecting any feature that wasn't already part of stages 1–8

## Plan

- [ ] Discuss with user: feature-parity audit, deprecation timing,
      vantage-live fate
- [ ] Audit Vista coverage of all old-wrapper use cases; produce a
      checklist of "parity confirmed" / "parity gap" / "explicit
      non-goal"
- [ ] Delete `vantage-table/src/any.rs`
- [ ] Delete or replace `vantage-table/src/traits/table_like.rs` (the
      old dyn-safe trait the wrapper boxed)
- [ ] Delete legacy `AnyTable` trait at `vantage/src/sql/table.rs`
- [ ] Delete or shrink `vantage-live` crate
- [ ] Restore `vantage-table/tests/table_like.rs` as Vista-flavoured
      tests
- [ ] Restore inline wrapper tests as Vista tests
- [ ] Convert `MockTableSource` to `Value = ciborium::Value` (closes
      `../../TODO.md` follow-up entry)
- [ ] Make `ImTable` / `ImDataSource` generic over `Value` (closes
      `../../TODO.md` Architecture entry)
- [ ] Update `bakery_model3` examples (CLI, scripts) to Vista
- [ ] Sweep `bakery_model4` (currently excluded from workspace) — bring
      into Vista or document permanent exclusion
- [ ] Sweep `example_*` crates for old types — these are sibling
      crates; coordinate per memory note "Stay within scope"
- [ ] Update CHANGELOG entries for affected crates
- [ ] Update `../../README.md`, `../../ARCHITECTURE.md` if they describe
      the old types

## References

- Closes:
  - `../../TODO.md` "AnyTable CBOR-swap follow-up" sub-tree:
    - Convert `MockTableSource` to `Value = ciborium::Value`
    - Restore `vantage-table/tests/table_like.rs`
    - Restore inline `AnyTable` tests
    - `bakery_model4` sweep
    - MongoDB / CSV CBOR fidelity (already addressed in stage 4)
  - `../../TODO.md` "Architecture: Make ImTable / ImDataSource generic
    over Value"
- Touches:
  - `../../TODO.md` Trait boundary fixes — most are absorbed by Vista's
    surface; remaining ones become standalone follow-ups after this
- Removes:
  - The legacy `vantage/src/sql/table.rs::AnyTable` trait (unrelated to
    the new struct, dead today, becomes unambiguous workspace-wide)
