# Changelog

## 0.4.1 — 2026-05-18

- New BDD test harness under `vantage-diorama/tests/` using [cucumber](https://crates.io/crates/cucumber). Scenarios run on a single-threaded paused-clock tokio runtime so refresh/timeout behaviour is deterministic — `tokio::time::advance` is the only thing that moves the clock.
- Initial features cover the Phase-1 mock-backend wiring (`skeleton.feature`) and the three Lens lifecycle contracts (`lifecycle.feature`): `on_start_blocking=true` parks `make_dio`, `on_start_blocking=false` lets it return, and dropping the last `Dio` handle shuts the write worker down cleanly.
- Added `#[doc(hidden)]` `Dio::take_write_worker_handle` so the harness can await clean worker exit. Not part of the supported surface — `#[doc(hidden)]` is the contract.

## 0.4.0 — 2026-05-18

- Initial release. `vantage-diorama` adds a cached, composable, reactive surface in front of a `vantage-vista` `Vista`: `Dio::vista()` hands callers a fresh facade Vista that reads through the cache, while writes go to the master and re-emit through the event bus.
- Designed to pair with the schema-on-source `TableShell` shape introduced in [vantage-vista 0.4.10](https://docs.rs/vantage-vista/0.4.10/vantage_vista/): the facade shell forwards `columns` / `references` / `id_column` to the master, so consumers don't see a stale or duplicated schema.
- Pre-release: API surface, scenery types, and event-bus semantics will move before 0.5.
