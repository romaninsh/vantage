# Stage 2 — First driver integration

Status: **Done**

Pick one driver and wire it through end-to-end: typed `Table<T, E>` →
Vista via that driver's factory, executing a real `list` query. Validates
the trait shape from stage 1 against a real backend.

## Discussion phase

Confirmed with the user:

- [x] First driver: **vantage-csv**. CSV stores everything as `String`,
      which makes the CBOR conversion boundary easy to inspect, and
      sample fixture rows already live under `vantage-csv/data/`.
- [x] Bridge approach: factory in the driver crate consumes typed
      `Table<Csv, E>` and produces a `Vista`. `vantage-table` already
      exposed every accessor we needed (`columns()`, `id_field()`,
      `title_fields()`, `table_name()`); no additions required.
- [x] CBOR translation lives in the driver — vantage-csv already had
      `From<AnyCsvType> for ciborium::Value` for the `AnyTable` path,
      and vista reuses it at the `VistaSource` boundary.
- [x] vantage-vista is opt-in via a `vista` cargo feature on
      vantage-csv; the existing TableSource path is unaffected.

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

- [x] Discuss with user: which driver, which accessors to add to
      vantage-table
- [x] In `vantage-table`: no additions needed — `Table::columns`,
      `id_field`, `title_fields`, `table_name` already public
- [x] In `vantage-csv`: `pub fn vista_factory(&self) -> CsvVistaFactory`
      (gated behind the `vista` feature)
- [x] Implement `VistaFactory::from_table` for `CsvVistaFactory`
- [x] Implement `VistaSource` for `CsvVistaSource` — read path
      (`list`, `get`, `get_some`, `count`) returns CBOR; writes return
      "CSV is a read-only data source"
- [x] CBOR translation: reused existing `From<AnyCsvType>` impls at
      the Vista boundary (`csv_record_to_cbor` + eq-condition matcher)
- [x] Capability declaration — `can_count: true`; everything else
      `false` (CSV is read-only)
- [x] Round-trip integration test in `vantage-csv/tests/vista.rs` —
      typed Table → Vista → list/get/count, eq-condition filtering,
      write-error advertising
- [x] Typed `Table<Csv, E>` API unchanged
- [x] Convert the csv branch of `bakery_model3/examples/cli.rs` to
      drive a Vista — preserves identical command surface, validates
      the API on real fixtures

## References

- Subsumes:
  - `../../TODO.md` "Architecture: Make ImTable / ImDataSource generic
    over Value" (partial — drivers no longer need a JSON middleman)
- Touches:
  - `../../TODO.md` "Convert MockTableSource to Value = ciborium::Value"
    (test fixture needs CBOR; addressed cleanly in stage 9)
