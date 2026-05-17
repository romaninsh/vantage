# Changelog

## 0.4.9 — 2026-05-17

- `SurrealTableShell` ships the full Stage 5 query surface: [`add_order`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_order) on any column (every column gets the [`ORDERABLE`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/flags/constant.ORDERABLE.html) flag), [`add_search`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_search) via the existing `search_table_condition`, and offset-style pagination ([`set_page_size`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.set_page_size) + [`fetch_page`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.fetch_page) / [`fetch_next`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.fetch_next), encoding the cursor as a 1-based page number).
- `SurrealDB::search_table_condition` now actually fans out across columns — OR of case-insensitive `string::contains(string::lowercase(<string>field), needle)` for every column, instead of the SEARCH-stub placeholder. Drives `Vista::add_search` end-to-end.
- Capabilities updated: `can_order`, `can_search`, `can_set_page_size`, `can_fetch_page`, `can_fetch_next` all `true`. The retired `paginate_kind` flag is gone — drop it from any direct `VistaCapabilities` construction.
- Pins `vantage-vista = "0.4.9"`, `vantage-table = "0.4.12"`.

## 0.4.8 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.7 — 2026-05-16

- `SurrealTableShell` implements [`TableShell::get_ref`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/trait.TableShell.html#method.get_ref) and `get_ref_kinds`: row-based reference traversal at the Vista layer. The shell converts the CBOR parent row into `Record<AnySurrealType>`, calls `Reference::resolve_from_row` on the wrapped typed table, and re-wraps via `SurrealVistaFactory::from_table`.
- `SurrealDB::eq_value_condition` implemented via `SurrealOperation::eq`.
- Pins `vantage-vista = "0.4.7"`, `vantage-table = "0.4.10"`.

## 0.4.6 — 2026-05-14

- Drop the `arbitrary_precision` feature flag from the `serde_json` dependency. It enabled a global mode that wraps numbers as `{"$serde_json::private::Number": "..."}` objects, which broke ad-hoc JSON inspection and forced every consumer of vantage-surrealdb's `serde_json::Value` outputs to opt into the same flag. `preserve_order` and `raw_value` are retained.

## 0.4.5 — 2026-05-10

New opt-in [`vista`](https://docs.rs/vantage-surrealdb/0.4.5/vantage_surrealdb/vista/struct.SurrealVistaFactory.html) feature: build a [`Vista`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/struct.Vista.html) from a typed `Table<SurrealDB, E>` or from YAML, with full read+write CRUD and server-side `eq` push-down.

- `SurrealDB::vista_factory()` returns a [`SurrealVistaFactory`](https://docs.rs/vantage-surrealdb/0.4.5/vantage_surrealdb/vista/struct.SurrealVistaFactory.html); `from_table` and `from_yaml` both produce a `Vista`. `from_table` preserves the entity type so `with_expression` closures still typecheck.
- YAML `surreal:` block carries `table` (override the SurrealDB table name); per-column `surreal: { field }` aliases the field name. The `thing` / `record` column type maps to [`Thing`](https://docs.rs/vantage-surrealdb/0.4.5/vantage_surrealdb/thing/struct.Thing.html); `datetime` and `decimal` round-trip via `AnySurrealType`.
- Vista's string id boundary translates `"table:id"` straight to `Thing`; bare ids get prefixed with the wrapped table's name, so `vista.get_value("biff")` works the same as `vista.get_value("client:biff")`.
- [`TableShell::add_eq_condition`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/trait.TableShell.html#method.add_eq_condition) translates `(field, CborValue)` into `column.eq(value)` and pushes onto the wrapped table — `WHERE` is server-side.
- Capabilities: `can_count`, `can_insert`, `can_update`, `can_delete` all true. `can_subscribe` deferred to a later live-query pass.
- Off by default; non-vista users still don't depend on `vantage-vista`.
- Bug fix: `get_table_value` now treats `SELECT FROM ONLY ... NONE` (CBOR `Tag(6, _)`) as "no row" alongside the existing `Null` case, so missing-record lookups consistently return `None` instead of erroring downstream.

## 0.4.4 — 2026-05-09

- Pins `vantage-types` to `>= 0.4.2` so cargo can't resolve back to a pre-`RichText` 0.4.x.

## 0.4.3 — 2026-05-09

- `TerminalRender for AnySurrealType` now returns `vantage_types::RichText` (with semantic
  `Style::Muted` / `Success` / `Error`) instead of `String` + a separate `color_hint()`, tracking
  the [`vantage-types 0.4.2`](https://docs.rs/vantage-types/0.4.2/) trait change.

## 0.4.2 — 2026-04-19

- Patch bump in the 0.4 release line.
