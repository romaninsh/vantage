# Stage 10 — Decommission vantage-live

Status: **Not started**

Once Diorama has reached parity with `vantage-live` (achieved by
stages 1–4 plus the GPUI adapter in stage 8), the `vantage-live` crate
is no longer load-bearing. This stage migrates the few remaining
consumers, deletes the crate's code (or shrinks it to a re-export
shim for one cycle), and updates documentation across the workspace.

This is the cleanup pass for the Diorama side. Vista's stage 9
references this stage for the `vantage-live` fate decision.

## Discussion phase

- [ ] Deprecation timing: hard delete on the Diorama release that
      ships stage 8, vs. one minor cycle of re-export shim with
      `#[deprecated]` attributes. Lean: shim for one cycle —
      out-of-tree consumers (if any) get a warning before the cut.
- [ ] Feature parity audit: enumerate every use case `vantage-live`
      supported; confirm each has a Diorama equivalent. Known
      mapping:
      | vantage-live concept     | Diorama equivalent                  |
      |--------------------------|-------------------------------------|
      | `LiveTable::new`         | `lens.make_dio(master)`             |
      | `Cache` trait            | redb-backed cache Vista (auto)      |
      | `MemoryCache`            | `MemorySource` cache backend        |
      | `with_custom_write_target` | `on_write` callback routes writes |
      | `with_live_stream`       | `on_event` + manual stream forward  |
      | `LiveStream` trait        | moved to vantage-diorama (stage 4) |
      | `LiveEvent`              | `ChangeEvent` (stage 4 rename)      |
- [ ] In-tree consumers of `vantage-live`:
  - `vantage-surrealdb` (LIVE wiring) — stage 4 already migrates this
  - `vantage-ui` — has it ever depended on `vantage-live` directly,
    or only via vantage-ui-adapters? Verify before this stage starts
  - Any examples or docs that import `vantage-live::LiveTable`
- [ ] Out-of-tree consumers: unknown but likely zero (vantage-live
      shipped recently). The deprecation cycle catches anyone who's
      adopted it.
- [ ] `vantage-live`'s `live_demo.rs` example and the
      `insert-client-every-second.sh` helper script — migrate to
      `vantage-diorama/examples/` or delete? Lean: migrate to
      `examples/live_demo.rs` in vantage-diorama; keep the helper
      script for manual testing.

## Scope

In:

- Migration of remaining `vantage-live` consumers (in-tree) to
  `vantage-diorama`
- Deletion of `vantage-live`'s `live_table/` (LiveTable, worker,
  event_consumer, write_op) — all subsumed by Diorama
- Deletion of `vantage-live`'s `cache/` module — Diorama uses
  `Arc<dyn TableSource>` (Vista's existing pattern)
- Optionally: `vantage-live` crate becomes a thin re-export shim with
  `#[deprecated]` attributes for one cycle, then deleted in the
  cycle after
- Move `live_demo.rs` example into `vantage-diorama/examples/`
- Update `../../TODO.md` to mark the LiveTable-related entries closed
- Update `../../CHANGELOG.md` (root) noting the supersession
- Update `../../README.md` (root) if it mentions vantage-live

Out:

- New Diorama features in this stage — pure cleanup
- Decommissioning of vista's `AnyTable` — that's vista stage 9, a
  separate concern

## Plan

- [ ] Discuss with user: deprecation cycle (hard cut vs. shim),
      example migration, helper-script fate
- [ ] Audit in-tree consumers of `vantage-live`:
      `grep -r "vantage-live\|vantage_live" --include="Cargo.toml"
      --include="*.rs"` across the workspace
- [ ] For each consumer, write the Diorama equivalent: typically a
      Lens + `make_dio` + the appropriate callbacks
- [ ] Migrate `vantage-live/examples/live_demo.rs` to
      `vantage-diorama/examples/live_demo.rs`:
  - Replace `LiveTable::new(master, cache_key, cache)` with
    `lens.make_dio(master)`
  - Replace `with_live_stream(stream)` with a `tokio::spawn` forwarder
    into `dio.handle_event(evt)`
  - Same CLI surface, same demo capability
- [ ] Decide and execute on `vantage-live` crate fate:
  - Option A: hard delete after migration; remove from workspace
    members; close `vantage-live/` directory
  - Option B: shrink to a re-export shim crate that re-exports
    `vantage-diorama` types with `#[deprecated]` notices; delete
    the next cycle
- [ ] Update root `../../TODO.md`: close the
      "Wire up real LIVE query support" sub-tree (most of it closed
      by stage 4; this stage closes the remaining example/helper
      bullets)
- [ ] Update root `../../CHANGELOG.md`: entry noting `vantage-live`
      retirement and the migration mapping
- [ ] Sweep `bakery_model3/examples/` for any `vantage-live`
      references (probably none, but check)
- [ ] Sweep `example_*/` sibling crates for `vantage-live`
      dependencies — coordinate per "Stay within scope" memory note
      (separate PRs per crate)
- [ ] Update `../../vantage-vista/plans/9-decommission.md` reference:
      tick the "Delete or shrink vantage-live" bullet as closed by
      this stage

## References

- Closes:
  - `../../TODO.md` "Wire up real LIVE query support" remaining sub-bullets
    (the demo example + helper script bullets; the core wiring closed
    in stage 4)
- Subsumes:
  - The entire `vantage-live` crate as a load-bearing component
- Touches:
  - `../../vantage-vista/plans/9-decommission.md` — references this
    stage for the vantage-live fate decision; pair the closeout
