# Stage 2 — First driver integration

Status: **Not started**

Pick one driver and wire it through end-to-end: typed `Table<T, E>` →
Vista via that driver's factory, executing a real `list` query. Validates
the trait shape from stage 1 against a real backend.

## Discussion phase

- [ ] Pick first driver. Recommendation: **vantage-sqlite** (simple,
      well-understood, no async runtime quirks). Alternative:
      **vantage-mongodb** (already exercises type translation deeply).
- [ ] Confirm the bridge approach: factory in the driver crate consumes
      typed `Table<T, E>` and produces a `Vista`. `vantage-table` is
      *not* a god-struct — it gains only minimal accessors (typed column
      iteration) needed by drivers.
- [ ] Confirm CBOR translation lives in the driver, not in vantage-table
      or vantage-vista.
- [ ] Confirm what vantage-table accessors drivers need (column kinds,
      flags, refs definitions) — minimise additions.

## Scope

In:

- One driver implements `VistaFactory::from_table(typed) -> Vista`
- Minimal vantage-table accessor additions (read-only, surface-only)
- CBOR translation: column types ↔ `CborValue`, ids, records
- End-to-end test: typed Table → Vista → `list()` returns CBOR rows

Out:

- YAML loading (stage 3)
- Other drivers (stage 4)
- Conditions beyond what the driver natively does (stage 5)

## Plan

- [ ] Discuss with user: which driver, which accessors to add to
      vantage-table
- [ ] In `vantage-table`: add minimal typed-column accessor methods
      drivers need (no behaviour change, just visibility)
- [ ] In chosen driver crate: `pub fn vista_factory(&self) -> XVistaFactory`
- [ ] Implement `VistaFactory::from_table` for that driver
- [ ] Implement `VistaSource` for the driver — `list`, `get`, `count`
      sufficient for first slice; rest stub with `not implemented`
- [ ] CBOR translation: per-type bridges in the driver
- [ ] Capability declaration — populate `VistaCapabilities` honestly
- [ ] Round-trip integration test: typed Table → Vista → list → assert
      CBOR shape
- [ ] Ensure typed `Table<T, E>` is unchanged in API; only added accessors

## References

- Subsumes:
  - `../../TODO.md` "Architecture: Make ImTable / ImDataSource generic
    over Value" (partial — drivers no longer need a JSON middleman)
- Touches:
  - `../../TODO.md` "Convert MockTableSource to Value = ciborium::Value"
    (test fixture needs CBOR; addressed cleanly in stage 9)
