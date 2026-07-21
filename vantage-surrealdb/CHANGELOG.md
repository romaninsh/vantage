# Changelog

## 0.6.6 — 2026-07-21

- Implicit references (`Table::with_active_columns`) lower into a native
  SurrealQL idiom path via the new `TableSource::traversal_path_expr` hook —
  each segment escaped on its own so `batch.golf_course.name` traverses the
  record links rather than looking up one `⟨batch.golf_course.name⟩` literal
  field. Multi-hop comes for free. The vista factory advertises
  `can_traverse_in_columns`.

## 0.6.5 — 2026-07-20

- `expr:` computed columns: a column's Rhai script now lowers into a
  server-side `(<expr>) AS <name>` projection, including traversal through
  record links (e.g. `ident("batch")["name"]`). The Rhai indexer that builds
  those paths (`t["col"]`) now escapes each segment separately instead of
  joining them into one literal identifier, so `batch.name` renders as a
  real traversal rather than a single escaped field name.
- `get_table_value` (single-record reads) now runs the table's own narrowed
  `select` instead of a bare `SELECT * FROM ONLY <id>`, so `expr:` columns
  project on the detail read path too, not just lists.
- A query-sourced table can key rows by a plain scalar (a `GROUP BY month`
  aggregate's id is text like `"2025-08"`); those rows now synthesize a
  `Thing` from the table name instead of being dropped for lacking a
  record-id-shaped id field.
- An `eq` condition on the id column coerces a string value (bare key or
  `table:key`) into a `Thing` before comparing — SurrealDB compares record
  ids by type, so a quoted string never matched. This is what lets a
  cross-persistence relation narrow a surreal table from a plain string held
  by another backend's row.

## 0.6.4 — 2026-07-16

- CBOR→JSON conversion is the shared `vantage-types` walker under a local
  `SurrealDialect`; tag rendering unchanged (record ids, compact datetimes/durations,
  UUIDs, decimals). `From<JsonValue>` no longer carries an `.expect()` panic path, and
  the commented-out legacy converters are gone.

## 0.6.3 — 2026-07-16

- Fixed `AnySurrealType`'s JSON conversion dropping CBOR tags. `From<AnySurrealType> for
  serde_json::Value` previously round-tripped through a plain `ciborium`/`serde` re-encode, which
  silently discards CBOR tags — a datetime (`Tag(12, [seconds, nanos])`, SurrealDB's compact
  encoding) surfaced as the raw `[seconds, nanos]` array instead of a timestamp, and record ids,
  UUIDs, decimals, and durations were similarly unformatted whenever this path was hit. The
  conversion now understands SurrealDB's tags directly, rendering the same textual form SurrealDB's
  own JSON API produces (an RFC 3339 string for datetimes, `table:id` for record ids, etc.).

## 0.6.2 — 2026-07-16

- Live subscriptions. `SurrealTableShell` implements `watch_vista` over
  surreal-client's new `LIVE SELECT`, mapping each `CREATE`/`UPDATE`/`DELETE`
  to a fine-grained `VistaChange`; a real-table vista advertises `can_subscribe`.
  Each create/update re-reads the row through the same `get_vista_value` path the
  initial snapshot uses, so a live update is byte-identical to the listing.
- Fixed `replace` (and so `ActiveEntity::save` on a fresh record) silently doing
  nothing. Since SurrealDB 2.0 a plain `UPDATE` on a non-existent record is a
  no-op; `replace`'s contract is create-if-missing, so it now renders `UPSERT`
  (new `SurrealUpdate::upsert()`).

## 0.6.1 — 2026-06-25

- `AnySurrealType` declares `null_when: ciborium::Value::Null`, so it implements `InvariantValue`
  (vantage-types 0.6.2) and participates in vantage-table's set-invariant enforcement.

## 0.6.0 — 2026-06-17

- Coordinated 0.6 release; internal dependencies realigned to 0.6. No changes beyond 0.5.12.

## 0.5.12 — 2026-06-13

- `Identifier` escaping is now correct on SurrealDB 3.x. It picks up the `surreal-client` 0.5.2 fix:
  a `⟩` inside a column/field name is emitted as `\u{27E9}` (the previous `\⟩` was an invalid escape
  that failed to parse) and backslashes are doubled, closing an identifier-injection hole where a
  crafted `\⟩` could break out of `⟨…⟩` quoting. Verified against a live SurrealDB.

## 0.5.11 — 2026-06-13

- `similarity(expr, term)` and `time_group(expr, unit)` now bind their literal token as a query
  parameter instead of interpolating it into a single-quoted SurrealQL literal. A `term`/`unit`
  containing a `'` previously broke out of the literal and could inject SurrealQL — relevant because
  `similarity` is typically fed a runtime search term and both are exposed through the Rhai config
  layer. The token is now routed through the same CBOR `$_arg` binding as every other scalar.

## 0.5.10 — 2026-06-13

- `Identifier` escaping now routes through `surreal-client`'s shared `escape_identifier`, removing a
  weaker duplicate that didn't escape an embedded `⟩` (which could break out of `⟨…⟩` quoting) and
  only handled spaces and a short keyword list. Column/field names with special characters, leading
  digits, or empty names are now escaped consistently with record-id rendering.
- `Fx` now emits its built-in function name (e.g. `string::lowercase`) verbatim instead of through
  `Identifier`, so the qualifying `::` is no longer mistakenly `⟨…⟩`-quoted.

## 0.5.9 — 2026-06-07

### Added

- SurrealDB vendor vocabulary for the new Rhai-scripted reference traversal (requires the
  `rhai` feature, which now also implies `vista` + `vantage-vista/rhai`).
  - Per-reference `surreal: { rhai: "…" }` block (`SurrealReferenceExtras` / `SurrealRefBlock`):
    a script that builds the traversal target — conditions (including vendor expressions like
    `ident("client") == row.id`), order, and search — evaluated lazily with the parent `row`
    in scope, instead of the default foreign-key eq-condition path.
  - `surreal: { modify: "…" }` on a table block: a Rhai script applied to the *built* vista as
    a final step (exposed as `self`), so a script can narrow it with vendor expressions YAML
    can't represent. Composes with `table`/`rhai`/`base` and runs last.
  - `SurrealTableShell` overrides `register_rhai_extensions` (registers the Surreal engine
    vocabulary plus a `with_condition(<expr>)` builtin) and implements `add_raw_condition`,
    routing a boxed backend-native `Expr` through the type-erased condition path.

### Changed

- `register_surreal_engine!` is split so its registrations live in a reusable
  `register_surreal_onto(&mut Engine)`; the macro and `__create_engine` call it (no behavior
  change for existing engine call sites).

## 0.5.8 — 2026-06-03

- Query-sourced and derived vistas for SurrealDB, mirroring the SQL-backend features from PR #277:
  - `Table::from_select(db, alias, select)` creates a read-only table whose `FROM` clause is an
    arbitrary sub-`SELECT` instead of a physical table name.
  - `Table::derive_from(&base, alias, modifier, cols, rels)` derives a new table from an existing
    one, inheriting columns and relations via shared `Arc` refs. The modifier closure transforms the
    base `select()` (e.g. adding a `WHERE`).
  - YAML `rhai:` source — a vista backed by a Rhai-built `SurrealSelect` (read-only).
  - YAML `base:` + `inherit:` — derive from another vista by name, with optional `rhai:` transform
    (base select seeded into scope as `base` variable).
  - `clear_fields()` Rhai method — drops inherited fields for valid `GROUP BY` in aggregation
    transforms.
  - `SurrealTableBlock` gains `rhai`, `base`, and `inherit` fields; new `InheritBlock` struct for
    `columns`/`relations` selection.
  - Read-only flag: rhai-sourced and base-derived vistas have `can_insert`, `can_update`,
    `can_delete` set to `false`.

## 0.5.7 — 2026-06-02

- Tables can now be sourced from a sub-`SELECT` via `vantage-table`'s new `SelectSource`
  (`type Source = SelectSource<SurrealSelect>`), rendering `FROM (<select>) AS <alias>`.

## 0.5.6 — 2026-06-03

- Rhai expression primitives for building SurrealDB queries from scripts. Tier 1 shared vocabulary
  (`count`, `sum`, `avg`, `round`, `coalesce`, `cast`, `nullif`, `case_when`, `date_format`) plus
  Surreal-specific Tier 2 primitives (stats/collection fns, `graph()`/`recurse()`/`me`,
  `group_all`/`split`/`subquery`, `param`). Field paths via `ident("t")["col"]` with numeric and
  string indexers. Q5 closures (`.map`/`.fold`/`.filter`) take native Rhai `|l|` closures that bind
  to placeholder `$name` expressions and emit SurrealQL symbolically. 10-query golden suite
  (`v4_q01..v4_q10` + `v4_param`) renders byte-identical to normalized v4 statements via
  `examples/rhai_test`.

## 0.5.5 — 2026-06-02

- Rhai scripting DSL for building SurrealDB queries. Self-contained `rhai_engine` module with
  wrapper types, constructors, comparison operators, and select builder methods. Supports basic
  SELECT, SurrealDB-specific features (ONLY, VALUE, graph traversal via `arrow`/`back`), record IDs
  (`thing`), `$parent` references, and SurrealDB-namespaced aggregates (`count`, `math::sum`,
  `math::min`, `math::max`). Gated behind optional `rhai` feature flag. Includes golden file test
  runner with 4 smoke tests.

## 0.5.4 — 2026-05-31

- SurrealDB v2 seed defines order lifecycle status and cancellation fields.

## 0.5.3 — 2026-05-31

- Contained relations backed by native nested objects and arrays: an order's embedded `lines`
  surface as an editable sub-Vista and can traverse out to real tables (`line.product`). See the
  [contained relations guide](https://romaninsh.github.io/vantage/new-persistence/step9-contained-relations.html).

## 0.5.2 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.0 — 2026-05-23

- Bumped to the 0.5 line to track
  [vantage-table 0.5.0](https://docs.rs/vantage-table/0.5.0/vantage_table/)'s opening of the
  `AnyTable` decommission cycle. No code changes beyond the dependency pin.

## 0.4.11 — 2026-05-23

- YAML `references:` blocks now build live traversals. `SurrealVistaFactory::build_from_spec`
  registers each `has_one` / `has_many` entry as a `with_one` / `with_many` on the underlying
  [`Table`](https://docs.rs/vantage-table/0.5.0/vantage_table/table/struct.Table.html), so
  `vista.get_ref("clients", &row)` returns a fully-typed child Vista — no Rust glue per relation.
- New
  [`SurrealSpecResolver`](https://docs.rs/vantage-surrealdb/0.4.11/vantage_surrealdb/vista/type.SurrealSpecResolver.html)
  (`Arc<dyn Fn(&str) -> Option<SurrealVistaSpec>>`) plus
  [`SurrealVistaFactory::with_resolver`](https://docs.rs/vantage-surrealdb/0.4.11/vantage_surrealdb/vista/struct.SurrealVistaFactory.html#method.with_resolver):
  pass in a name-to-spec lookup and child tables get their columns from the resolved spec at
  traversal time, not from a single pre-built registry.
- Many-to-many drops out of chained `has_many` / `has_one` traversal — no new YAML keyword needed.
  See `tests/7_vista_refs.rs` for `client → bakery → clients`.
- Without a resolver the references still parse and surface in
  [`VistaMetadata`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/struct.VistaMetadata.html),
  but traversed children come back column-less; the next query then fails loudly.

## 0.4.10 — 2026-05-18

- Tracks [vantage-vista 0.4.10](https://docs.rs/vantage-vista/0.4.10/vantage_vista/)'s
  schema-on-source refactor. `SurrealTableShell` now owns its
  [`VistaMetadata`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/struct.VistaMetadata.html)
  and implements the new `columns` / `references` / `id_column` shell methods.
  `surrealdb.vista_factory().from_table(...)` / `from_yaml(...)` surface unchanged.
- Pins `vantage-vista = "0.4.10"`.

## 0.4.9 — 2026-05-17

- `SurrealTableShell` ships the full Stage 5 query surface:
  [`add_order`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_order)
  on any column (every column gets the
  [`ORDERABLE`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/flags/constant.ORDERABLE.html)
  flag),
  [`add_search`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_search)
  via the existing `search_table_condition`, and offset-style pagination
  ([`set_page_size`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.set_page_size) +
  [`fetch_page`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.fetch_page)
  /
  [`fetch_next`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.fetch_next),
  encoding the cursor as a 1-based page number).
- `SurrealDB::search_table_condition` now actually fans out across columns — OR of case-insensitive
  `string::contains(string::lowercase(<string>field), needle)` for every column, instead of the
  SEARCH-stub placeholder. Drives `Vista::add_search` end-to-end.
- Capabilities updated: `can_order`, `can_search`, `can_set_page_size`, `can_fetch_page`,
  `can_fetch_next` all `true`. The retired `paginate_kind` flag is gone — drop it from any direct
  `VistaCapabilities` construction.
- Pins `vantage-vista = "0.4.9"`, `vantage-table = "0.4.12"`.

## 0.4.8 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.7 — 2026-05-16

- `SurrealTableShell` implements
  [`TableShell::get_ref`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/trait.TableShell.html#method.get_ref)
  and `get_ref_kinds`: row-based reference traversal at the Vista layer. The shell converts the CBOR
  parent row into `Record<AnySurrealType>`, calls `Reference::resolve_from_row` on the wrapped typed
  table, and re-wraps via `SurrealVistaFactory::from_table`.
- `SurrealDB::eq_value_condition` implemented via `SurrealOperation::eq`.
- Pins `vantage-vista = "0.4.7"`, `vantage-table = "0.4.10"`.

## 0.4.6 — 2026-05-14

- Drop the `arbitrary_precision` feature flag from the `serde_json` dependency. It enabled a global
  mode that wraps numbers as `{"$serde_json::private::Number": "..."}` objects, which broke ad-hoc
  JSON inspection and forced every consumer of vantage-surrealdb's `serde_json::Value` outputs to
  opt into the same flag. `preserve_order` and `raw_value` are retained.

## 0.4.5 — 2026-05-10

New opt-in
[`vista`](https://docs.rs/vantage-surrealdb/0.4.5/vantage_surrealdb/vista/struct.SurrealVistaFactory.html)
feature: build a [`Vista`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/struct.Vista.html) from
a typed `Table<SurrealDB, E>` or from YAML, with full read+write CRUD and server-side `eq`
push-down.

- `SurrealDB::vista_factory()` returns a
  [`SurrealVistaFactory`](https://docs.rs/vantage-surrealdb/0.4.5/vantage_surrealdb/vista/struct.SurrealVistaFactory.html);
  `from_table` and `from_yaml` both produce a `Vista`. `from_table` preserves the entity type so
  `with_expression` closures still typecheck.
- YAML `surreal:` block carries `table` (override the SurrealDB table name); per-column
  `surreal: { field }` aliases the field name. The `thing` / `record` column type maps to
  [`Thing`](https://docs.rs/vantage-surrealdb/0.4.5/vantage_surrealdb/thing/struct.Thing.html);
  `datetime` and `decimal` round-trip via `AnySurrealType`.
- Vista's string id boundary translates `"table:id"` straight to `Thing`; bare ids get prefixed with
  the wrapped table's name, so `vista.get_value("biff")` works the same as
  `vista.get_value("client:biff")`.
- [`TableShell::add_eq_condition`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/trait.TableShell.html#method.add_eq_condition)
  translates `(field, CborValue)` into `column.eq(value)` and pushes onto the wrapped table —
  `WHERE` is server-side.
- Capabilities: `can_count`, `can_insert`, `can_update`, `can_delete` all true. `can_subscribe`
  deferred to a later live-query pass.
- Off by default; non-vista users still don't depend on `vantage-vista`.
- Bug fix: `get_table_value` now treats `SELECT FROM ONLY ... NONE` (CBOR `Tag(6, _)`) as "no row"
  alongside the existing `Null` case, so missing-record lookups consistently return `None` instead
  of erroring downstream.

## 0.4.4 — 2026-05-09

- Pins `vantage-types` to `>= 0.4.2` so cargo can't resolve back to a pre-`RichText` 0.4.x.

## 0.4.3 — 2026-05-09

- `TerminalRender for AnySurrealType` now returns `vantage_types::RichText` (with semantic
  `Style::Muted` / `Success` / `Error`) instead of `String` + a separate `color_hint()`, tracking
  the [`vantage-types 0.4.2`](https://docs.rs/vantage-types/0.4.2/) trait change.

## 0.4.2 — 2026-04-19

- Patch bump in the 0.4 release line.
