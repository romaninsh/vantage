# Stage 1 — vantage-diorama crate skeleton

Status: **Done**

Set up the new crate with type definitions and trait surfaces only. No
execution logic. No cache reads. No callback dispatch. Output:
workspace compiles and `vantage-diorama` exposes the public API shape
the later stages will build on.

This mirrors vantage-vista's stage 1 — shape-only, no behavior.

## Discussion phase

Confirm with the user:

- [x] `Lens` struct fields — `cache_source: Arc<dyn TableSource>`,
      `callbacks: Arc<LensCallbacks>`, `defaults: LensDefaults`,
      `runtime: tokio::runtime::Handle`. Lock the visibility of each
      field (most should be `pub(crate)` with accessors).
- [x] `LensCallbacks` shape — five callback slots (`on_start`,
      `on_refresh`, `on_write`, `on_event`, `on_query`), each
      `Option<...>` with the HRTB type aliases. Confirm we hold five
      independent boxes, not one trait-object that bundles them.
- [x] HRTB callback signature — verified pattern is
      `Box<dyn for<'a> Fn(&'a Dio) -> Pin<Box<dyn Future<Output =
      Result<()>> + Send + 'a>> + Send + Sync>`. Confirm this compiles
      against the Rust toolchain in use (works on stable since 1.75,
      but the in-crate ergonomics depend on the toolchain).
- [x] `LensDefaults` fields — `refresh_interval: Option<Duration>`,
      `cache_ttl: Option<Duration>`, `write_queue_capacity: usize` (256),
      `on_start_blocking: bool` (true). Confirm naming and defaults.
- [x] `Dio` shape — `Arc<DioInner>` wrapper, clone-cheap. `DioInner`
      holds `lens`, `master`, `cache`, channels, hot tier. Confirm:
      does `Dio` itself implement Clone (yes — internal Arc) or is
      `Arc<Dio>` the right surface?
- [x] `DioShell` placeholder — empty struct holding `Arc<DioInner>`,
      `impl TableShell` returns `Unsupported` for everything. Real
      delegation lands in stage 2.
- [x] `WriteOp` enum shape — `Insert(Record<CborValue>)`,
      `Update(RecordId, Record<CborValue>)`, `Delete(RecordId)`,
      `Replace(RecordId, Record<CborValue>)`. Confirm vocabulary
      matches Vista's verbs.
- [x] `QueryDescriptor` placeholder — exact fields for `on_query`
      callbacks deferred to stage 5 (where Sceneries actually issue
      queries). For stage 1, an opaque newtype.
- [x] `ChangeEvent` enum — `Updated { id, new }`, `Inserted { id }`,
      `Deleted { id }`, `Invalidated`. Confirm shape mirrors what
      vantage-live's `LiveEvent` carries.
- [x] `Generation` newtype around `u64` — used by Sceneries; lock the
      newtype now even if stage 5 is what consumes it.
- [x] `DioEvent` enum — the bus message Sceneries subscribe to.
      `RecordChanged { id }`, `Invalidated`, `Refreshing`,
      `WriteFailed { id, error }`. Confirm distinct from `ChangeEvent`
      (which is the upstream-facing shape).
- [x] Cargo features — none for v1. Confirm. (Possible future feature:
      `gpui` enabling the adapter glue. Out of scope here.)
- [x] Cache backend default — `Arc<dyn TableSource>` is generic, but
      `Lens::new()` needs *some* default. Either require an explicit
      `cache_at(path)` / `cache_source(...)` before `.build()`, or
      default to an in-memory backend. Lean: require explicit; the
      `cache_at(path)` is a one-liner.

## Scope

In:

- New `vantage-diorama` crate at workspace root
- `Lens` struct definition (no methods that touch the cache yet)
- `LensBuilder` struct with all setter methods (no dispatch yet)
- `LensCallbacks` struct + the five callback type aliases
- `LensDefaults` struct + `Default` impl
- `Dio` / `DioInner` struct shapes (no methods that fire callbacks)
- `DioShell` placeholder implementing `TableShell` with stubs
- `WriteOp`, `QueryDescriptor`, `ChangeEvent`, `DioEvent` enum shapes
- `Generation` newtype
- Empty trait shapes for `TableScenery`, `RecordScenery`, `ValueScenery`
  (methods declared, no impls anywhere)
- Composition module placeholders (`Diorama::overlay`,
  `Diorama::merge` return `unimplemented!()` for now)
- Error model — `LensBuildError`, `DioError` thin shells around
  `vantage_core::Error`

Out:

- All execution logic (stages 2+)
- Hot tier (moka) — stage 5
- Refresh scheduler — stage 3
- Write queue worker — stage 3
- Event bus broadcast wiring — stage 4
- Scenery state machinery — stages 5–7
- Composition implementations — stage 9

## Plan

- [x] Create `vantage-diorama/` crate directory structure
- [x] Add to root `Cargo.toml` workspace members
- [x] `Cargo.toml`: deps `vantage-core`, `vantage-types`, `vantage-vista`,
      `vantage-table`, `ciborium`, `indexmap`, `async-trait`,
      `tokio` (sync features for now), `thiserror`. No redb or moka
      yet — pulled in by stage 2 and stage 5 respectively.
- [x] Define `Lens` struct (fields, no logic-bearing methods)
- [x] Define `LensBuilder` struct + all `.with_*` setter methods +
      `.build()` returning `Result<Lens, LensBuildError>` (returns
      `Ok` after asserting required fields are present; no behavior)
- [x] Define `LensCallbacks` + the five callback type aliases
- [x] Define `LensDefaults` + `Default` impl
- [x] Define `Dio` (wraps `Arc<DioInner>`, derive Clone) and `DioInner`
- [x] Define `DioShell` placeholder + `impl TableShell` returning
      `Unsupported` for every method
- [x] Define `WriteOp` enum
- [x] Define `QueryDescriptor` placeholder newtype
- [x] Define `ChangeEvent` enum
- [x] Define `DioEvent` enum
- [x] Define `Generation` newtype + `From<u64>` / `Into<u64>`
- [x] Define `TableScenery` / `RecordScenery` / `ValueScenery` trait
      shapes with method signatures only
- [x] Composition module: `Diorama::overlay` / `Diorama::merge`
      placeholders returning `unimplemented!()`
- [x] Error enums (`LensBuildError`, `DioError`)
- [x] `cargo check --workspace` passes
- [x] No unit tests yet; this is shape-only

## References

- Touches:
  - `../../vantage-live/` — patterns to mirror for worker / event
    consumer split when stage 3/4 lands; nothing imported yet
  - `../README.md` — overview that mentions every type defined here
  - `../ARCHITECTURE.md` — detailed shape doc; types defined here
    match what's described there
