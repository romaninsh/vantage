# Changelog

## 0.4.0 — 2026-04-26

First 0.4-line release. The crate was commented out of the workspace through the 0.4 type-system rewrite; this version replaces the 0.3 `RecordEdit` / `RwValueSet` design with a focused write-through cache around `AnyTable`. **Public API and storage shape are not compatible with 0.3** — there is no in-place upgrade.

- New `LiveTable` wrapping any `AnyTable` master. Implements `ValueSet` / `ReadableValueSet` / `WritableValueSet` / `TableLike` — no new public dataset traits. Drop-in replacement anywhere code already speaks `Record<ciborium::Value>`.
- `Cache` trait with `MemCache` and `NoCache` impls. `RedbCache` is on the roadmap; the trait is shaped for it.
- `LiveStream` trait with a built-in `ManualLiveStream` (broadcast-channel) for tests. Each event invalidates the entire `cache_key` prefix; `Insert/Update/Delete{id}` variants are forward-compatible with surgical invalidation.
- Internal `mpsc` write queue + worker task: public `insert_value` / `replace_value` / `patch_value` / `delete` / `delete_all` post and await a `oneshot` reply. Cache invalidation runs inside the worker on success only.
- `tracing` instrumentation at five span boundaries (read, write, queue worker, event consumer, cache ops). `RUST_LOG=vantage_live=debug` shows the full hit/miss/invalidate dance.
- `examples/live_demo.rs` — self-contained CLI (redb-as-master, clap, ANSI colour timing). `cargo run --example live_demo -- --help` for the full surface.
- Pins `vantage-table = "0.4.6"` for `AnyTable::from_table_like`, the new constructor needed to wrap `LiveTable` (which is `TableLike` but not a `Table<T, E>`).
- Drops the 0.3 `RecordEdit` / snapshot-dirty-tracking surface entirely. If that concern lands again, it'll be a separate crate consuming `LiveTable` as a regular `TableSource`.
