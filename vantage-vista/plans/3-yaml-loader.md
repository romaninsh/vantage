# Stage 3 — Universal YAML loader

Status: **Not started**

Add YAML → Vista construction. Universal vocabulary (table name,
columns, flags, references) is parsed by `vantage-vista`; driver-specific
extras are delegated to the factory.

## Discussion phase

- [ ] Confirm `VistaSpec` shape — universal fields only
- [ ] Confirm where driver-specific YAML lives — top-level key per driver
      (e.g. `mongo:`, `aws:`, `sqlite:`) vs nested `extras:` block
- [ ] Confirm flag vocabulary: `id`, `title`, `searchable`, `mandatory`,
      `hidden` — extensible via driver?
- [ ] Confirm reference kinds: `has_one`, `has_many`, `has_foreign`?
- [ ] Confirm error reporting strategy for malformed YAML (line numbers,
      field paths)

## Scope

In:

- `VistaSpec` struct (universal columns, references, flags)
- `VistaFactory::from_yaml(yaml: &str) -> Result<Vista>`
- Per-driver `parse_extras(spec, &driver_block)` hook
- Cache key derivation (auto from `{datasource}/{name}`)
- Error reporting with field paths

Out:

- Hooks block (stage 6)
- Conditions block (stage 5)
- Coop integration (stage 7)

## Plan

- [ ] Discuss with user: VistaSpec shape, extras placement
- [ ] Define `VistaSpec` struct + serde derive
- [ ] Define universal `Flag` enum
- [ ] Define `ColumnSpec` (name, type, flags, references field)
- [ ] Define `ReferenceSpec` (name, target, kind, foreign_key)
- [ ] Implement `VistaFactory::from_yaml` default impl that:
      1. parses `VistaSpec`
      2. invokes `parse_extras` on the factory
      3. constructs Vista with the produced source
- [ ] First driver (from stage 2) implements `parse_extras` for its
      driver-specific YAML block
- [ ] Test: YAML round-trip with the first driver — load YAML, list rows,
      verify schema matches
- [ ] Document the YAML schema in a `SCHEMA.md` next to this plan

## References

- Subsumes:
  - `../../PLAN_0_5.md` §1 "Column visibility + column defaults" —
    flag vocabulary lands here, defaults later
  - `../../PLAN_0_5.md` §2 "Column (de)serialisation mapping" — surfaces
    in YAML type vocabulary
  - vantage-ui's per-driver `XColumnExtras` blocks — replaced by this
    extras pattern
