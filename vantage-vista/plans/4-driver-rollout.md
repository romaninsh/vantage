# Stage 4 — Driver rollout

Status: **In progress** — vantage-csv done in stage 3, vantage-mongodb done.
Remaining: vantage-surrealdb, vantage-aws, vantage-rest.

Roll out Vista support to remaining drivers. Each driver is its own
sub-discussion — different backends have different gotchas and may
declare different capabilities.

## Pattern from CSV + MongoDB

The two completed drivers settled the cross-driver shape that later
drivers should follow. Authoritative writeup is in
`docs4/src/new-persistence/step8-vista.md`; quick recap:

- A `vista` cargo feature gates the bridge so non-vista users don't pull
  in `vantage-vista`.
- `<Driver>::vista_factory()` inherent method as the entry point;
  returns a `<Driver>VistaFactory` wrapping the configured data source.
- `from_table<E>(Table<Driver, E>) -> Result<Vista>` — typed entry path.
  Collapses the entity to `EmptyEntity`, harvests column metadata,
  builds the source.
- `VistaFactory::build_from_spec` — builds a `Table<Driver, EmptyEntity>`
  from the spec, then wraps it through the same source-creation code as
  `from_table`. One construction path, one reading path.
- Driver extras live under a top-level key named after the driver
  (`csv:`, `mongo:`, …); column extras live under the same key inside
  each column entry. All driver blocks set `deny_unknown_fields` to
  catch typos.
- The source converts native id type → `String` and native value type
  → `CborValue` at the boundary.
- **Conditions never live on Vista.** `add_eq_condition` translates the
  CBOR pair into the driver's native condition type and pushes it onto
  the wrapped table — server-side filter push-down is automatic.
- **Nested fields use `column_paths: IndexMap<String, Vec<String>>`.**
  Source walks paths on read, rebuilds sub-docs on write, uses
  dot-notation (or backend equivalent) on filter. MongoDB ships this
  via `mongo: { nested_path: address.city }`.

The driver-level file layout the in-tree drivers converged on:

```
<driver>/src/vista/
├── mod.rs       re-exports + <Driver>::vista_factory() inherent impl
├── spec.rs      <Driver>TableExtras / <Driver>ColumnExtras / <Driver>VistaSpec
├── factory.rs   <Driver>VistaFactory + impl VistaFactory + spec→table helpers
├── source.rs    <Driver>TableShell + impl TableShell
└── cbor.rs      native ↔ CBOR bridge (only when native value type ≠ JSON-shaped)
```

## Discussion phase

Per driver, confirm:

- [ ] Native id type → `CborValue` translation (composite keys, opaque
      ids, binary ids)
- [ ] Native value type → `CborValue` translation (lossy paths flagged)
- [ ] Capability declaration (`can_count`, `paginate_kind`, etc.)
- [ ] Driver-specific YAML extras vocabulary
- [ ] Read-only vs read-write
- [ ] Any backend quirks that should not leak into universal YAML
      (e.g. AWS's `narrow_via` — should be in the driver's extras, not
      in the universal `ReferenceSpec`)

## Scope

In:

- vantage-mongodb factory + source
- vantage-surrealdb factory + source
- vantage-aws factory + source
- vantage-rest factory + source (replaces JsonToCborAdapter from
  vantage-ui)
- vantage-csv factory + source (read-only `can_write: false`)

Out:

- Portable conditions (stage 5) — drivers translate only what they
  natively support for now
- Hooks (stage 6)
- LIVE-query integration for SurrealDB (stage 7, with Coop)

## Plan

### vantage-csv — done (in stage 3)

- [x] Read-only — `can_count: true`, all writes return `Unsupported`.
- [x] `csv:` table block carries `path`; `csv: { source }` per column for
      header rename.
- [x] `add_eq_condition` builds `column.eq(any_csv_value)` and pushes to
      `Table.add_condition` — the table's existing `apply_condition`
      machinery does the filtering.
- [x] Identity id (`String`) at the boundary; no translation needed.
- [x] Tests in `tests/vista.rs` and `tests/vista_yaml.rs` — return
      `Result` and propagate via `?`.

### vantage-mongodb — done

- [x] ObjectId-as-id vs string fallback — `MongoId` already handles both
      via `FromStr` (24-char hex parses as `ObjectId`, otherwise `String`).
      The vista boundary stringifies via `MongoId::to_string()`
      (hex for ObjectId, raw for String) and parses back via `FromStr`.
      Decision: keep both `MongoId` variants for now; the
      "Drop the String variant from MongoId" TODO is independent.
- [x] **Condition delegation shipped early** (originally deferred to
      stage 5). `add_eq_condition` builds `doc!{path: bson_value}` and
      pushes to `Table.add_condition` — server-side filter push-down via
      Mongo's existing `find` filter. Vista carries no condition state.
- [x] `MongoVistaFactory` + `MongoTableShell` (read + write).
- [x] Driver extras: `mongo:` block with `collection` only.
      `read_preference` / `index_hints` deferred — they're knobs the
      universal layer doesn't surface yet, and adding empty fields now
      would be backwards-compat dead weight.
- [x] Column extras: `mongo: { field }` for single-level rename and
      `mongo: { nested_path: address.city }` for nested-doc projection.
      `column_paths: IndexMap<String, Vec<String>>` on `MongoTableShell`
      drives read/write/filter consistently.
- [x] BSON ↔ CBOR bridge in `vista/cbor.rs` with unit tests covering
      scalar + nested round-trips (lossy paths flagged in module docs).
- [x] Integration tests in `tests/6_vista.rs` — gated on `feature = "vista"`,
      requires a running MongoDB exactly like the existing TableSource
      tests. Uses `Result<(), Box<dyn Error>>` so `?` covers both
      `mongodb::error::Error` and `vantage_core::Error`.

### vantage-surrealdb

- [ ] Discuss: RecordId handling at the CBOR boundary
- [ ] Implement `SurrealVistaFactory` + `SurrealTableShell`
- [ ] Driver extras: `surreal:` block
- [ ] Note: LIVE-query subscription deferred to stage 7
- [ ] Integration test

### vantage-aws

- [ ] Discuss: composite-key handling (partition + sort) — is the id a
      single CBOR map?
- [ ] Discuss: where the magic `array_key:service/target` table addressing
      moves (driver extras, not universal `table:`)
- [ ] Discuss: `narrow_via` field — into driver extras for references
- [ ] Implement `AwsVistaFactory` + `AwsTableShell`
- [ ] Integration test

### vantage-rest

- [ ] Discuss: which condition operators a REST source can express via
      query params; which must be rejected at construction time
- [ ] Discuss: pagination (offset vs cursor) — per-table or per-driver?
- [ ] Implement `RestVistaFactory` + `RestTableShell` — CBOR-native end
      to end (no `serde_json::Value` middleman)
- [ ] Integration test


## References

- Subsumes:
  - `../../TODO.md` "MongoDB / CSV CBOR fidelity" — addressed natively
    per driver (no JSON middle step)
  - `../../TODO.md` "Add a sample CSV table implementation"
  - `../../FINAL_TODO.md` "In-process SQL-dialect-faithful mock" — out
    of scope here but driver pattern enables future addition
- Touches:
  - `../../TODO.md` "Drop the String variant from MongoId" — discuss in
    MongoDB sub-stage
  - `../../TODO.md` "Wire up real LIVE query support end-to-end" —
    deferred to stage 7
