# Changelog

## 0.4.1 — 2026-05-03

- WebSocket message handlers (`ws`, `ws_cbor`, `ws_pool`) wrap their spawned futures with `tracing::Instrument::in_current_span()` so connection events stay attached to the trace that opened the connection.
- Replaced `println!` / `eprintln!` in the WS handlers with `tracing::debug!` / `warn!` so consumers' subscribers see them. `tracing` becomes a direct dependency.
- The JSON `WsEngine` no longer panics on malformed inbound payloads — bad JSON, missing `id`, and unexpected frames now log at `warn` / `debug` and the message is dropped instead of taking the entire message-handler task down.

## 0.4.0 — 2026-04-16

- Initial 0.4 release. Standalone SurrealDB client over WebSocket with CBOR transport — used by `vantage-surrealdb` and usable on its own.
