# Changelog

## 0.5.3 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.2 — 2026-05-23

- Drops the `vantage_table::any::AnyTable` re-export from `prelude` — `AnyTable` is deleted upstream
  in [vantage-table 0.5.2](https://docs.rs/vantage-table/0.5.2/vantage_table/). Use
  `MongoDB::vista_factory().from_table(...)` for cross-driver wrapping.

## 0.5.1 — 2026-05-23

- Tracks [vantage-dataset 0.5.0](https://docs.rs/vantage-dataset/0.5/vantage_dataset/)'s `ImTable` /
  `ImDataSource` parametrization. No public API change in this crate.

## 0.5.0 — 2026-05-23

- Bumped to the 0.5 line to track
  [vantage-table 0.5.0](https://docs.rs/vantage-table/0.5.0/vantage_table/)'s opening of the
  `AnyTable` decommission cycle. No code changes beyond the dependency pin.

## 0.4.11 — 2026-05-18

- Tracks [vantage-vista 0.4.10](https://docs.rs/vantage-vista/0.4.10/vantage_vista/)'s
  schema-on-source refactor. `MongoTableShell` now owns its
  [`VistaMetadata`](https://docs.rs/vantage-vista/0.4.10/vantage_vista/struct.VistaMetadata.html)
  and implements the new `columns` / `references` / `id_column` shell methods.
  `mongodb.vista_factory().from_table(...)` / `from_yaml(...)` surface unchanged.
- Pins `vantage-vista = "0.4.10"`.

## 0.4.10 — 2026-05-17

- `MongoTableShell` ships the full Stage 5 query surface:
  [`add_order`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_order)
  on any column (MongoDB sorts on any field — every column gets the
  [`ORDERABLE`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/flags/constant.ORDERABLE.html)
  flag at construction),
  [`add_search`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.add_search)
  via the existing `search_table_condition`, and offset-style pagination
  ([`set_page_size`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.set_page_size) +
  [`fetch_page`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.fetch_page)
  /
  [`fetch_next`](https://docs.rs/vantage-vista/0.4.9/vantage_vista/struct.Vista.html#method.fetch_next),
  encoding the cursor as a 1-based page number).
- Capabilities updated: `can_order`, `can_search`, `can_set_page_size`, `can_fetch_page`,
  `can_fetch_next` all `true`. The retired `paginate_kind` flag is gone — drop it from any direct
  `VistaCapabilities` construction.
- Pins `vantage-vista = "0.4.9"`, `vantage-table = "0.4.12"`.

## 0.4.9 — 2026-05-16

- `MongoTableShell` implements
  [`TableShell::get_ref`](https://docs.rs/vantage-vista/0.4.7/vantage_vista/trait.TableShell.html#method.get_ref)
  and `get_ref_kinds`: row-based reference traversal at the Vista layer. The shell converts the CBOR
  parent row into `Record<AnyMongoType>`, delegates to `Reference::resolve_from_row` on the wrapped
  typed table, and re-wraps via `MongoVistaFactory::from_table`.
- `MongoDB::eq_value_condition` implemented — builds `doc! { field: bson_value }` directly via
  `AnyMongoType::to_bson`, sidestepping the `Expression → MongoCondition` coercion that previously
  needed the panic-stub `From` impl.
- Pins `vantage-vista = "0.4.7"`, `vantage-table = "0.4.10"`.

## 0.4.8 — 2026-05-09

- Pins `vantage-types` to `>= 0.4.2` for consistency with the other backends after the
  `TerminalRender → RichText` migration.

## 0.4.7 — 2026-05-04

- Implements
  [`TableShell::driver_name`](https://docs.rs/vantage-vista/0.4.4/vantage_vista/trait.TableShell.html#method.driver_name)
  — `Vista::driver()` reports `"mongodb"` for collections wrapped through
  `MongoDB::vista_factory()`.
- Bumps minimum [`vantage-vista`](https://docs.rs/vantage-vista/0.4.4/) requirement to 0.4.4.

## 0.4.6 — 2026-05-04

- `MongoVistaSource` is now
  [`MongoTableShell`](https://docs.rs/vantage-mongodb/0.4.6/vantage_mongodb/struct.MongoTableShell.html),
  tracking the
  [`vantage-vista 0.4.3`](https://docs.rs/vantage-vista/0.4.3/vantage_vista/trait.TableShell.html)
  trait rename. The factory's surface is unchanged — `MongoDB::vista_factory().from_table(...)` and
  `from_yaml(...)` both still produce a `Vista`.

## 0.4.5 — 2026-05-04

New opt-in
[`vista`](https://docs.rs/vantage-mongodb/0.4.5/vantage_mongodb/struct.MongoVistaFactory.html)
feature: build a [`Vista`](https://docs.rs/vantage-vista/0.4.2/vantage_vista/struct.Vista.html) from
a typed `Table<MongoDB, E>` or from YAML, with full read+write CRUD, server-side `eq` filtering, and
nested-document column projection.

- `MongoDB::vista_factory()` returns a
  [`MongoVistaFactory`](https://docs.rs/vantage-mongodb/0.4.5/vantage_mongodb/struct.MongoVistaFactory.html);
  `from_table` and `from_yaml` both produce a `Vista`.
- YAML `mongo:` block carries `collection`. Per-column `mongo: { field }` renames a single BSON key,
  `mongo: { nested_path: address.city }` projects out of nested sub-documents — reads walk the path,
  writes rebuild sibling sub-docs, filters use dot-notation.
- BSON ↔ CBOR bridge in `vista::cbor`. Lossy paths (`ObjectId`, `DateTime`, `Decimal128`, `Regex`)
  collapse to their string representation; documented in module docs.
- Capabilities: `can_count`, `can_insert`, `can_update`, `can_delete` all true. `can_subscribe`
  deferred to change-streams work.
- YAML validation: empty `nested_path: ""` and empty segments (`a..b`, `.a`, `a.`) now error at spec
  load with the offending column named, so the mistake doesn't surface later as a malformed BSON
  filter.
- Off by default; non-vista users still don't depend on `vantage-vista`.

## 0.4.4 — 2026-04-25

- `From`/`Into<ciborium::Value>` impls on `AnyMongoType` so MongoDB tables can be wrapped via
  `AnyTable::from_table`. Round-trips via `serde_json::Value` (Bson and ciborium are both
  serde-friendly; same lossy bits as the existing JSON bridge).
- Pins `vantage-table = "0.4.4"` to keep the pair in lock-step.

## 0.4.3 — 2026-04-19

- Reference traversal now bridges `ObjectId` and `String` id-field boundaries via
  `related_in_condition`'s dual push.
- `impl From<MongoId> for AnyMongoType` so `c.id().eq(MongoId::parse(...))` dispatches to the right
  BSON type.
