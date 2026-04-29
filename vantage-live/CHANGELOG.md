# Changelog

## 0.4.2 — 2026-04-29

- `LiveTable::get_ref(relation)` now forwards through to the master `AnyTable`, so reference traversal works on a `LiveTable` the same way it does on the underlying table — `live.get_ref("orders")` returns an `AnyTable` you can keep wrapping.
- Pins `vantage-table = "0.4.7"` for the new `AnyTable::get_ref` / `TableLike::get_ref` surface.

## 0.4.1 — 2026-04-26

- New `RedbCache::open(folder)` cache backend. Persists cached rows on disk so cache state survives process restarts. One redb file inside the folder, one redb table per `cache_key` (namespaced `__vlive__{cache_key}`) so the hot `invalidate_prefix(cache_key)` path is just a `delete_table` call — O(1)-ish, no scan. Sub-prefix invalidation falls back to scan-and-delete inside the table.
- `examples/live_demo.rs` reworked into two master modes: `local` (redb-backed master, full read/write/event cycle, same as 0.4.0) and `api <users|posts|comments|…>` (read-only JSONPlaceholder master, demonstrates the cache benefit dramatically — first call ~300ms over the network, second call sub-millisecond from the cache). Pagination is pushed into the API URL via `--page` / `--limit` flags, each page caches under its own key. `--filter field=value` adds an eq-condition that becomes a URL query param and folds into the cache_key — different filters cache under different keys, matching the caller-owned-cache-key contract.
- `--cache <PATH>` now uses `RedbCache` for real (was falling back to `MemCache` in 0.4.0). Combined with `api`, runs survive process restarts: `cargo run --example live_demo -- --cache ./vlive-cache api users list` — second invocation is microseconds-fast even though it's a fresh process.
- `vantage-redb` and `vantage-api-client` moved from runtime deps to dev-deps — runtime never needed either (the `RedbCache` impl uses raw `redb` directly; vantage-api-client is only used by the demo).

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
