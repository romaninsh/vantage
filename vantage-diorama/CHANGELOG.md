# Changelog

## 0.4.0 — 2026-05-18

- Initial release. `vantage-diorama` adds a cached, composable, reactive surface in front of a `vantage-vista` `Vista`: `Dio::vista()` hands callers a fresh facade Vista that reads through the cache, while writes go to the master and re-emit through the event bus.
- Designed to pair with the schema-on-source `TableShell` shape introduced in [vantage-vista 0.4.10](https://docs.rs/vantage-vista/0.4.10/vantage_vista/): the facade shell forwards `columns` / `references` / `id_column` to the master, so consumers don't see a stale or duplicated schema.
- Pre-release: API surface, scenery types, and event-bus semantics will move before 0.5.
