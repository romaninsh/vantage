# Stage 9 — Composition primitives

Status: **Not started**

Implement the `Diorama::` namespace's multi-Vista composition: `overlay`,
`merge`. Each returns a single value implementing the Vista interface
(typically by wrapping a `TableShell` over multiple inner Vistas), so
`lens.make_dio(composed_vista)` is unchanged. Composition is orthogonal
to caching — the Lens treats a composed Vista the same as a leaf Vista.

This unlocks the read-only-CSV + write-memory pattern and the
primary/fallback pattern explicitly described in the dialogue.

## Discussion phase

- [ ] `Diorama::overlay(base, overlay)` shape: reads merge both
      (overlay wins for shared ids), writes always go to overlay.
      Confirm. Use case: read-only CSV + writable in-memory shim →
      a read-write Vista.
- [ ] `Diorama::merge(primary, fallback)` shape: reads try primary,
      fall through to fallback on miss; writes go to primary only.
      Confirm. Use case: local cache + remote API.
- [ ] How do reads merge for `list_values` on overlay? Three options:
      (a) overlay rows replace base rows by id (full merge); (b)
      append overlay after base (no deduplication); (c) overlay rows
      only (overlay is a true overlay, base is masked). Lean: (a) —
      most useful behavior; matches the read-only-CSV-with-edit
      pattern.
- [ ] Capability union rules: a `merge(primary, fallback)` reports
      `can_count = primary.can_count || fallback.can_count`?
      `can_insert = primary.can_insert`? Confirm — propose a table:
      | Capability      | overlay(base, ov)       | merge(prim, fb)        |
      |-----------------|--------------------------|-------------------------|
      | can_count       | ov.can_count OR base    | prim.can_count OR fb   |
      | can_insert      | ov.can_insert            | prim.can_insert         |
      | can_update      | ov.can_update            | prim.can_update         |
      | can_delete      | ov.can_delete            | prim.can_delete         |
      | can_order       | both can_order          | both can_order         |
      | can_search      | both can_search          | both can_search        |
      | can_subscribe   | either                  | either                  |
      | can_fetch_page  | strict — needs design   | strict — needs design  |
- [ ] Pagination through composition: page cursors are
      driver-specific. Composed Vistas can't sensibly forward a
      cursor. Lean: composed Vistas advertise
      `can_fetch_page = false` even if inner Vistas can; users
      relying on paginated reads should compose at the Lens level
      (e.g. one Dio per inner Vista, app-level orchestration).
      Confirm.
- [ ] Condition handling: `add_condition_eq(field, value)` on a
      composed Vista applies to both inner Vistas. Confirm.
- [ ] Live events from composed Vistas: subscribe to both inner
      streams, multiplex into one. Confirm. (The composed Vista
      doesn't have its own change source — it's a pass-through.)
- [ ] Three-way composition: do we ship `Diorama::overlay(a,
      overlay(b, c))` working? It should, because composition produces
      a Vista which can be composed again. Confirm.

## Scope

In:

- `Diorama::overlay(base: Vista, overlay: Vista) -> Vista`
- `Diorama::merge(primary: Vista, fallback: Vista) -> Vista`
- Internal `OverlayVista` / `MergeVista` types with `TableShell` impls
- `list_vista_values` merges according to the rules above
- `get_vista_value(id)` checks overlay first (overlay), or primary
  first (merge)
- Writes routed per the rules above
- Capability union computed at construction time
- Conditions propagated to both inner Vistas
- Integration tests:
  - overlay(read_only_csv, in_memory_vista): read returns merged
    rows; write goes to in_memory; reading after write reflects the
    edit
  - merge(local_cache_vista, remote_api_vista): read tries cache
    first, falls through to remote on miss
  - Compose composed: `overlay(a, overlay(b, c))` works
  - Capability flags match the propose table
- Example: `examples/overlay_csv.rs` — read-only CSV + writable
  in-memory; demonstrate edits surviving in-process
- Example: `examples/merge_cache_remote.rs` — local cache + remote
  fallback

Out:

- Cross-Vista cache coherency (edits in overlay invalidating queries
  in base) — out of scope; user-level concern
- Three-way merge with conflict resolution — out of scope
- Distributed composition (Vistas across hosts) — out of scope

## Plan

- [ ] Discuss with user: merge semantics for `list_values`,
      capability union rules, pagination policy, three-way composition
- [ ] Implement `OverlayVista` struct + `TableShell` impl
- [ ] Implement `MergeVista` struct + `TableShell` impl
- [ ] Implement `Diorama::overlay` and `Diorama::merge` constructors
      returning `Vista` (the wrapper Vista contains
      `Box<OverlayVista>` / `Box<MergeVista>` as its shell)
- [ ] Capability union impl (per the table above)
- [ ] Condition propagation: `add_eq_condition(field, value)` on
      composed Vista delegates to both inner shells
- [ ] Live-event multiplex (if both inner Vistas advertise
      `can_subscribe`): merged stream
- [ ] Write integration tests
- [ ] Add `examples/overlay_csv.rs` and `examples/merge_cache_remote.rs`
- [ ] Update `../README_rust_dev.md` "Composition with other Vistas"
      section to reference real code (already drafted; verify
      consistency)

## References

- Subsumes:
  - `../README.md` mention of composition as one of the three things
    Diorama does — the third thing is real after this stage
  - `../README_rust_dev.md` "Composition with other Vistas" section
- Touches:
  - The dialogue's `OverlayDia` / `SortDia` / `FilterDia` /
    `CachingDia` / `RateLimitedDia` / `RetryDia` stack — overlay and
    merge here cover the multi-source primitives; sort/filter
    fallbacks are handled by Lens callbacks (not composition), and
    rate-limit/retry are user-Lens callbacks or middleware around the
    inner Vista, not Diorama framework concerns
