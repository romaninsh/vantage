# Changelog

## 0.6.0 — unreleased

- Coordinated 0.6 release; internal dependencies realigned to 0.6. No public API changes.

## 0.5.1 — 2026-06-06

### Changed

- Extracted `Relation::narrow` from `VistaCatalog::traverse`. Narrowing a freshly built
  target `Vista` from a parent row is now a reusable method, so callers that already hold
  a target can apply the relation's join conditions directly instead of going through a
  full `traverse`.

## 0.5.0 — 2026-06-06

### Added

- Initial release. `VistaCatalog` — a config- and driver-agnostic, registration-based
  name → `Vista` catalog with cross-persistence traversal. Register a model loader per
  table name with `register`, then `build_vista(name)` to materialize a `Vista` and
  `traverse(relation, parent_row)` / `traverse_from(...)` to resolve and narrow a related
  `Vista` regardless of which persistence backs it.
- `Relation` with `single_key` / `multi_key` constructors describing how a target `Vista`
  is narrowed from a parent row, plus `register_relation` to attach relations to the catalog.
