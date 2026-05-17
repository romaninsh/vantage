# Stage 8 — GPUI adapter

Status: **Not started**

Land the first concrete UI binding: GPUI. The adapter code lives in
`vantage-ui-adapters` (the existing crate that already hosts shared
types used by multiple UI integrations). Three small entities bridge
the three Scenery traits into GPUI's notification model, and a
reference `TableDelegate` example shows the full path from Lens
configuration to rendered grid.

This stage also ports `vantage-ui`'s existing grid to use Diorama
instead of the current bulk-load pattern, validating the bindings
against a real app.

## Discussion phase

- [ ] Adapter crate home: `vantage-ui-adapters/src/diorama/` is the
      proposed module. Confirm — it's already where shared UI glue
      lives.
- [ ] Bridge entity scope: one per Scenery type? Lean: yes —
      `TableSceneryEntity`, `RecordSceneryEntity`, `ValueSceneryEntity`.
      They're nearly identical (each spawns a watch-listening task
      and calls `cx.notify`), so a macro might reduce duplication.
      Pick: macro vs explicit three structs. Lean: explicit — clearer
      for adapter authors copying the pattern for other frameworks.
- [ ] `gpui-component`'s `TableDelegate` requires exact row count and
      a synchronous `render_td`. Our `TableScenery::row(idx) ->
      Option<Arc<EnrichedRecord>>` is synchronous. For unloaded rows,
      we render a skeleton cell. Confirm the skeleton cell shape — is
      this a vantage-ui-adapters utility, or per-app?
- [ ] Sort interaction: gpui-component calls `perform_sort(col_ix,
      direction)`; the adapter delegates to
      `scenery.set_sort(field, dir)`. Confirm we expose
      `SortDirection::into() -> SortDir` (or similar) so the bridge
      is a one-liner.
- [ ] Quicksearch widget: input change → `scenery.set_search(query)`.
      Adapter ships a `SceneryQuicksearchInput` widget that handles
      the binding, or leaves it to apps? Lean: leave to apps —
      quicksearch UX varies (debounce, placeholder, clear-icon
      details). Adapter doc shows the pattern.
- [ ] Migration scope for vantage-ui: replace `RecordGrid` + detail
      sheet only, or sweep all grids in one go? Lean: replace one
      grid as a proof, leave the rest for a separate PR.
- [ ] Storybook: does `widget-storybook` need a `MockLens` /
      `MockDio` for offline fixtures? Lean: yes — ship a
      `MockDio::with_rows(rows)` constructor in
      vantage-ui-adapters/test-util so storybook entries can render
      without a real backend.

## Scope

In:

- `vantage-ui-adapters/src/diorama/mod.rs` with submodules:
  - `table_entity.rs` — `TableSceneryEntity` wrapping
    `Arc<dyn TableScenery>` + watch task
  - `record_entity.rs` — `RecordSceneryEntity` wrapping
    `Arc<dyn RecordScenery>` + watch task
  - `value_entity.rs` — `ValueSceneryEntity` wrapping
    `Arc<dyn ValueScenery>` + watch task
- A reference `TableDelegate` impl in
  `vantage-ui-adapters/examples/products_table.rs` that consumes a
  `TableSceneryEntity` and renders against `gpui-component::DataTable`
- Migration of `vantage-ui`'s `RecordGrid` to use the new adapter:
  - Replace `Vec<Value>` + bulk-load with `TableSceneryEntity` +
    `TableScenery::row(idx)`
  - Replace double-click → sheet pattern: open
    `dio.record_scenery(id)` instead of the one-shot
    `get_value(id)`
- `MockDio` test utility in `vantage-ui-adapters/test-util` for
  storybook fixtures
- Integration test: launch a small GPUI test app that mounts a
  TableSceneryEntity, simulates a `dio.invalidate_record(id)`, asserts
  the table re-renders (via gpui's test harness)

Out:

- Other UI framework adapters (cursive, egui, slint, tauri) — each
  follows the same pattern; left for follow-up
- Sweeping the rest of `vantage-ui`'s grids onto Diorama — one grid
  is enough to validate the pattern
- Quicksearch UI standardization — adapter doc shows the pattern;
  per-app choices remain per-app

## Plan

- [ ] Discuss with user: adapter crate home, macro vs explicit
      structs, migration scope, mock fixture shape
- [ ] Add `vantage-diorama` dep to `vantage-ui-adapters/Cargo.toml`
- [ ] Implement `TableSceneryEntity`:
  - Hold `Arc<dyn TableScenery>` + `Task<()>`
  - `new(scenery, cx)` spawns a task that awaits
    `scenery.subscribe().changed()` and calls `cx.notify()`
  - `scenery()` accessor
- [ ] Implement `RecordSceneryEntity` (mirror pattern)
- [ ] Implement `ValueSceneryEntity` (mirror pattern)
- [ ] Implement reference `TableDelegate`:
  - `rows_count` → `scenery.row_count()`
  - `render_td(row_ix, col_ix)` → match `scenery.row(row_ix)`:
    Some → render cell; None → skeleton cell
  - `has_more` → `scenery.has_more()`
  - `load_more` → `scenery.request_load_more()`
  - `visible_rows_changed(range)` → `scenery.set_viewport(range)`
  - `perform_sort(col_ix, dir)` → `scenery.set_sort(field, dir.into())`
- [ ] Migrate `vantage-ui`'s `RecordGrid` to consume
      `TableSceneryEntity`:
  - Replace bulk `list_values()` call with
    `lens.make_dio(vista).table_scenery().open()`
  - Replace `dataset.observe()` with
    `TableSceneryEntity::new(scenery, cx)`
- [ ] Migrate the detail sheet:
  - Double-click event → `dio.record_scenery(id)`
  - Sheet binds to `RecordSceneryEntity`
- [ ] Add `MockDio` and `MockLens` to
      `vantage-ui-adapters/test-util` for storybook fixtures
- [ ] Update storybook entries for the migrated grid + sheet to use
      `MockDio`
- [ ] Run integration test (gpui test harness) — assert refresh
      propagation
- [ ] Update `../README_ui.md` to reference real code paths in
      `vantage-ui-adapters/src/diorama/`

## References

- Subsumes:
  - `../README_ui.md` "The GPUI binding pattern" — code becomes real
  - `../README_ui.md` "A worked example end-to-end" — products grid
    + sheet is the migration target
- Touches:
  - `../../vantage-vista/plans/8-ui-migration.md` — that stage covers
    the broader vantage-ui migration onto Vista; this stage covers
    the Diorama-specific UI binding. They're adjacent: vista stage 8
    migrates the bulk model from `EntityBackend.columns` to Vista
    metadata; this stage migrates the reactive surface from manual
    `dataset.observe()` to Scenery.
- Pairs with:
  - `vantage-ui-adapters/src/diorama/` becomes the canonical home for
    UI integration code as more frameworks land
