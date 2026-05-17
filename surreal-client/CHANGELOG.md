# Changelog

## 0.4.3 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.4.2 — 2026-05-09

- `ws://` and `wss://` now route through the CBOR engine. `cbor://` remains as an alias.
- The JSON `WsEngine` is gone. CBOR is the only wire format.
- `Engine` trait: `send_message_cbor` is the real method; `send_message` is a default that transcodes JSON↔CBOR around it. `supports_cbor()` removed (always true).
- `SurrealClient::supports_cbor()` removed.
- `SurrealError::Rpc` variant removed.
- `RpcMessage`/`RpcResponse` types removed.
- New private `cbor_convert` module implements the JSON↔CBOR fallback used by the JSON convenience methods. SurrealDB recordids (Tag 8) are flattened to `"table:id"` strings; bytes are base64; other CBOR-only types are best-effort. Use `query_cbor` for full fidelity.

## 0.4.1 — 2026-05-03

- WebSocket message handlers (`ws`, `ws_cbor`, `ws_pool`) wrap their spawned futures with `tracing::Instrument::in_current_span()` so connection events stay attached to the trace that opened the connection.
- Replaced `println!` / `eprintln!` in the WS handlers with `tracing::debug!` / `warn!` so consumers' subscribers see them. `tracing` becomes a direct dependency.
- The JSON `WsEngine` no longer panics on malformed inbound payloads — bad JSON, missing `id`, and unexpected frames now log at `warn` / `debug` and the message is dropped instead of taking the entire message-handler task down.

## 0.4.0 — 2026-04-16

- Initial 0.4 release. Standalone SurrealDB client over WebSocket with CBOR transport — used by `vantage-surrealdb` and usable on its own.
