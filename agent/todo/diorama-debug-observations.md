# Observations from the diorama debug-stream work (PR #374)

Collected while building the DebugTap feature; none block the PR.

## Bugs found by running the debug stream (2026-07-30)

- **FIXED — faker advertised `can_order` but could not order.** `FakerTable::build`
  and `build_shaped` created a bare `MockShell` and handed the column list only to
  `FakerCtx` (value generation), never registering it as `VistaMetadata`. So the
  shell reported no columns, and `Vista::add_order` failed every sort with
  "Unknown column for add_order". Because the failure was instant and the rows
  never landed, each viewport move re-requested them — a tight retry loop that
  presents as the app hanging. Fixed by building `VistaMetadata` from the declared
  columns (orderable + searchable, carrying the YAML's own flags).
- **FIXED — a paged view kept stale row positions across a sort change.**
  `reseed_from_cache` rebuilt the index→row map from the cache, which for a paged
  view holds an arbitrary subset of the *previous* order. Sorting that subset
  locally placed it at rows 0..N of the new order, so the grid showed correct rows
  in wrong places, mixed with correctly-placed rows wherever a later fetch landed.
  Fixed by dropping positions (keeping the cached records) when
  `paged && master.can_order && sort.is_some()`, letting the loader refill.
- **The `grid` component has no quicksearch control**, so a datasource that
  advertises `search` cannot be searched from a `kind: grid` page — `kind: binder`
  is needed. Worth surfacing in the page docs; a grid over a searchable source
  silently offers less than the source can do.
- Consequence for the docs: the "source cannot search → filter locally" branch is
  **unreachable from the UI**, because the search box is hidden exactly when that
  branch would apply. It can only fire for a non-UI consumer.

## Bugs / suspicious behaviour

- **`EnvFilter` matches targets by raw string prefix**, with no `::` boundary check
  (`meta.target().starts_with(&target[..])`, tracing-subscriber directive.rs). vantage-ui's
  default filter is `info,vantage_ui=debug,…`, so `vantage_ui_components::*` and
  `vantage_ui_adapters::*` are silently switched to debug too — which is why the
  per-frame `grid_frames` tap floods a default run nobody asked to be verbose.
  Fixed the flood at the source (log only on phase change, see below); the filter
  itself is still catching more than it names, and `RUST_LOG=info` is the workaround.
- **`census: … uptime_ms=0` on the first line.** `debug::process_stats()` measures
  uptime from a `LazyLock<Instant>` initialised on *first call*, so the first census
  reports 0 while `cpu_ms=2625` proves the process had been up for seconds. It is
  "time since the first census", not process uptime. Either stamp the start eagerly
  when the tap is enabled, or rename the field to what it measures.
- **The exit summary is never emitted by vantage-ui.** `stats::emit_debug_summary()`
  exists and works, but nothing calls it on shutdown, so the documented
  `— diorama session summary —` block (fetch ledger, repeats, redundant rows) never
  appears in a real run. Needs a call on the app's graceful-quit path.
- `grid_dio::load_phase()` emitted its frame tap unconditionally, and is called
  several times per paint (skeleton gate, empty state, per rendered cell) — so it
  was N identical lines per frame. Now logs only on transition (fixed in the
  vantage-ui PR-2 work).

- `two_pass::diagnostics_report_sceneries_refcount_and_hydration` is flaky under
  full-parallel `cargo test` load (seen 3× across independent runs; always passes
  in isolation and on rerun). Pre-existing timing sensitivity — worth its own look.
- `LensDefaults::cache_ttl` is declared (defaults.rs) and settable via
  `LensBuilder::cache_ttl()` but has no read site anywhere in the crate — a dead
  knob users can set with zero effect. Either implement or remove.
- `AggregateLens::derive()` computes the initial aggregate twice: an eager
  pre-spawn compute, then the engine's seed pass over the same rows (visible in
  the debug stream as two `trigger="initial"` lines, `unchanged=false` then
  `true`). Real duplicate work, architectural — documented, not fixed.

## Debug-stream follow-ups (settle before the acceptance harness freezes greps)

- Seed `last_debug_state` with `Some(Loading)` so the first `state` line says
  `from="Loading"` instead of `from="None"`.
- Add a test for the `demanded=Some(set)` branch of the `"columns"` line
  (`undemanded_count` — the leak alarm) — testable in diorama alone via a
  scenery opened with a column demand; latest by the vantage-ui PR.
- The census stream can show a race-loser's `closed` before its `opened`
  (dedup race at scenery open) — document when census gets formal docs.
- `dio_name` on state/list lines is an open-time snapshot; census/loader lines
  read the live master name — can disagree after `Dio::reload`.

## Process notes

- CI's rustdoc job (`RUSTDOCFLAGS="-D warnings" cargo doc --document-private-items`)
  is not in the usual local preflight (test/clippy/fmt) — it caught two private
  intra-doc links this round. Add `cargo doc` to the preflight checklist.
- `faker-shapes` was local-only (never pushed); pushed as a branch to stack
  PR #374 on it — it still needs its own PR/merge decision.
