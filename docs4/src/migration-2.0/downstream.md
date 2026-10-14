# Downstream: vantage-ui and vantage-actions

This page is the work list for the port that follows 2.0: vantage-ui, its vantage-actions
dependency (through vantage-lib), the skills bundle and the example apps. It is grouped by area and
names files where the earlier phases found them. Nothing here is compiled against vantage-ui; the
other pages hold the snippets. The file paths are relative to each repository and were found by
read-only greps, so treat them as a starting point, not a complete list.

## vantage-actions

- `src/row.rs`, `read_only_row`: the resolver moves onto the handle.
  `RecordDraft::from_row(Handle::named("row"), Some(no_table), Writes::Denied(denial.into()), id, values)`
  becomes
  `RecordDraft::from_row(Handle::named("row").with_resolver(no_table), Writes::Denied(denial.into()), id, values)`.
- Any resolver closure it builds types its parameter: `Arc::new(|name: &str| ..)`. An untyped one
  fails with "implementation of `Fn` is not general enough".

## vantage-ui: resolvers and rhai

- `crates/app/vantage-ui/src/infra/actions/run/body.rs`: `BodyRow::Writable` builds
  `Handle::named(table)`, attaches the `Option<Arc<dyn VistaResolver>>` with
  `match resolver { Some(r) => handle.with_resolver(r), None => handle }`, and calls
  `RecordDraft::from_row(handle, Writes::Allowed, id, fields)`.
- Every `SpecResolver<X>` in type position is `Arc<dyn SpecResolver<X>>`, and every closure behind
  one is `|name: &str|`. vantage-lib's callers need the same.
- Quick filters (`quick-filters/script.rs`): `Handle::steps()` and `Step` are crate-private.
  Replace the steps loop with `let q = handle.query()?;` and derive the Dio with
  `dio.derive().with_query(&q).build().await?`. Quick filters refused `sort()` before; to keep
  that, reject `q.order.is_some()`. `ref()` and `limit()` errors now come from `query()`.
- Form submit and the wizard proxy: `vantage_diorama::rhai::{cbor_to_dynamic, dynamic_to_cbor,
  record_to_map}` are `vantage_vista::rhai::…`; `DataVocab::read(..)` and friends are
  `DataVocab::new(Terminals::ReadWrite { limit, writes }).with_resolver(r)`.
- Rhai `capabilities()` keys come from `Capability::iter()` (`insert`, `order`, `window`, …); the
  old `can_*` keys read as `()`.

## vantage-ui: Lens and Dio

- The lens module: `Lens::new()` / `LensBuilder::new()` → `Lens::builder()`; `cache_at`,
  `cache_in_memory`, `cache_source` → `.cache(Cache::…)`; the hooks → `Loader`, `Refresher`,
  `writer`; `LensBuildError` → the error's `setting` context; `make_dio` / `make_dio_as` →
  `lens.dio(vista).build().await?`.
- Its own cache TTL map → `Refresher::max_age(ttl)`.
- Hand-built `-where-` cache keys go: a derived Dio's cache table is `name?<query key>`. Old redb
  tables are orphaned; sweep them with `CacheBackend::list_tables` / `drop_table` if needed.
- `dio.vista()` → `dio.cached()` everywhere.
- Every place that narrowed a Dio (`with_condition_eq`, the mutating `with_order`, quick filters)
  becomes one `dio.derive()…build().await?`.
- `Dio::augment(resolver, ..)` → `lens.dio(v).augment(..)`, with each `Detail::Catalog(name)` as
  `Detail::Resolved { resolver, name }`; custom fetch closures take the listed row.
- Entity: `dio.watch()` → `.live()` on the builder.
- Inspection: `Dio::cache_table_name()` → `dio.diagnostics().await?.cache_table`.
- Grids over REST sources without `Order` or `Search` fail on sort or search until their lens gives
  the paged loader an `or_eager` start and a `Refresher`; otherwise show the failure.
- `!vista.can_fetch_page()`, used as "API-backed", is `!vista.capabilities().has(Capability::Window)`.

## vantage-ui: writes and events

- The crud widget and adapters: facade writes return once confirmed or rolled back; drop the
  polling loops and handle the returned error.
- `flash_insert` / `flash_patch` / `flash_delete` → `dio.flash(ChangeFlash::…)`; `handle_event`,
  `patched`, `notify_*`, `reconcile_values` → `apply_change` / `reconcile`.
- `subscribe_events` consumers handle `RecordInserted` and `RecordRemoved`, not only
  `RecordChanged`, and may see `LoadFailed` with an empty range from a live follow.
- Matches on `FlashKind`, `ChangeEvent` and `ServoStatus` (and every other diorama enum) need a
  `_ =>` arm.

## vantage-ui: sceneries and widgets

- `SortDir` → `SortDirection`; `OpCondition` → `Condition`; builder `where_eq` / `where_op` →
  `filter`, `sort` → `order`; `set_sort` → `set_order(Option<..>)`; `set_filter_terms` →
  `set_filters(Vec<Condition>)`; `set_shown_range` → the second range of `set_viewport`;
  `request_refresh` → `refresh_source`; `is_loading()` → `load_state() == LoadState::Loading`.
- App routing: `CappedScenery` / `open_tail` → builder `limit` / `tail`.
- App and form: `record_scenery(id).await`, `_sugared`, `_with` → `record_scenery(id)…open()`.
- The inspector: `SceneryDiagnostic::refcount` → `shared_by`.
- Show `LoadState::Failed(failure)` where a grid shows its empty state.
- Value sceneries and dashboards: `value_scenery().count()` and the `AggregateLens` paths →
  `value_scenery(Count::rows())`, `dio.aggregate(..)`, imported from `vantage_diorama::aggregate`.

## vantage-ui: paths, features and diagnostics

- `vantage_diorama::scenery::…` paths → the crate root.
- `shell/view_summary.rs` reads `debug::num`, the one formatter that stays public; `dur`, `bytes`
  and `pct` are crate-private.
- Enable `vantage-diorama/diagnostics` where the app opens the debug stream (`debug_datasource`),
  logs the session summary or reads `diagnostics()`.
- Rust `ShapedShell` and `map_shell` call sites (form-tester, select-tester, the grid's `dio.rs`,
  select-vendored): `pub source` is gone; wrap with `map_shell`.
- vantage-lib `slot.rs` and the test hosts: `vantage_memory::vista::Catalog` → `MemoryCatalog`.
- The csv-dio and sqlite-dio loaders: struct literals of `VistaSpec`, `ColumnSpec`,
  `ReferenceSpec` → `VistaSpec::new(name)`, `ColumnSpec::new()`.

## vantage-ui: YAML, scripts and the skills bundle

- Shape YAML capabilities: `fetch_next` → `cursor`, `fetch_window` → `window`; `count` and
  `fetch_page` are removed.
- The `Preview.driver` key is gone from serialised previews; the MCP preview adds `driver` from
  `vista.driver_name()` itself. The skills that describe preview output change with it.
- Record and servo `save()` → `flash()` in scripts and skills.
- SQL scripts (`rhai:` sources, `base:` transforms, `mysql: { modify }`, reference `rhai:`, select
  `query:` options) prefix each builder function with `sql::`; SurrealDB scripts with `surreal::`,
  `me` → `surreal::me`; MongoDB's `oid`, `date`, `doc` with `mongo::`. `table("x")` as an
  identifier is `sql::ident("x")`. A missed call fails at build with "Function not found".
- The skills bundle (`crates/domain/skills/resources/`, bump `metadata.version`): vantage-rhai,
  vantage-queries, vantage-dashboards, vantage-persistence-sqlite, -postgres, -mysql, -surrealdb,
  -rest, vantage-debugging, vantage-components and vantage-quality reference the old builder
  names, `can_*` capability keys or preview keys.
- vantage-ui-examples: `apps/breg-bakery/table/bakery-surreal/*.yaml`, `apps/cashgpt/table/*.yaml`
  and `apps/cashgpt/page/*.yaml`, `apps/real-world/table/*.yaml` use the global builder names.
- Has-many targets declare their foreign key column; a target that leaves it out fails traversal
  with an error naming the relation and the column.
- Flag the columns a search should read `searchable`.
- Untyped JSON columns under a `Where` aggregate need a declared type, or operands written in the
  cell's own type.
- Generated schema descriptions (`page-schema-1.json`, `table-schema-1.json`) come from doc
  comments; regenerate them after the port.

## vantage-lib

- Its `vista_loader/references.rs` lowers column `references:` sugar itself, naming the relation
  after the target (`author_id: { references: author }` gives `author`), where vantage-table and
  vantage-memory name it after the column (`author_id`). Pick one name before vantage-lib drops
  its copy for `vantage_table::table::spec::spec_references`.

## Checklist

- [ ] vantage-actions `read_only_row` carries the resolver on the handle
- [ ] vantage-ui compiles with no `vista()`, `make_dio`, `steps()` or `SortDir`
- [ ] every diorama enum match in vantage-ui has a `_ =>` arm
- [ ] the skills bundle and the example YAML use `flash()`, namespaced builders and the new
      capability keys, and `metadata.version` is bumped
- [ ] the cache TTL map and `-where-` keys are deleted
