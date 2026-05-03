# Stage 4 — Driver rollout

Status: **Not started**

Roll out Vista support to remaining drivers. Each driver is its own
sub-discussion — different backends have different gotchas and may
declare different capabilities.

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

### vantage-mongodb

- [ ] Discuss: ObjectId-as-id vs string fallback (cross-reference
      `../../TODO.md` "Drop the String variant from MongoId")
- [ ] Discuss: condition translation — which native MongoDB filter
      operators are translated, which deferred
- [ ] Implement `MongoVistaFactory` + `MongoVistaSource`
- [ ] Driver extras: `mongo:` block with `collection`, `read_preference`,
      `index_hints`
- [ ] Integration test against bakery_model3 / a fixture

### vantage-surrealdb

- [ ] Discuss: RecordId handling at the CBOR boundary
- [ ] Implement `SurrealVistaFactory` + `SurrealVistaSource`
- [ ] Driver extras: `surreal:` block
- [ ] Note: LIVE-query subscription deferred to stage 7
- [ ] Integration test

### vantage-aws

- [ ] Discuss: composite-key handling (partition + sort) — is the id a
      single CBOR map?
- [ ] Discuss: where the magic `array_key:service/target` table addressing
      moves (driver extras, not universal `table:`)
- [ ] Discuss: `narrow_via` field — into driver extras for references
- [ ] Implement `AwsVistaFactory` + `AwsVistaSource`
- [ ] Integration test

### vantage-rest

- [ ] Discuss: which condition operators a REST source can express via
      query params; which must be rejected at construction time
- [ ] Discuss: pagination (offset vs cursor) — per-table or per-driver?
- [ ] Implement `RestVistaFactory` + `RestVistaSource` — CBOR-native end
      to end (no `serde_json::Value` middleman)
- [ ] Integration test

### vantage-csv

- [ ] Discuss: file path resolution, column-type inference
- [ ] Implement `CsvVistaFactory` + `CsvVistaSource`
- [ ] Declare `can_write: false`, `can_count: true`, `can_subscribe: false`
- [ ] Integration test
- [ ] Sample CSV table referenced in `../../TODO.md` "Add a sample CSV
      table implementation" — close that item here

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
