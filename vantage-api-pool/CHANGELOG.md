# Changelog

## 0.1.2 — 2026-05-03

- HTTP worker pool, request/response matcher, and all paginator variants now wrap their spawned futures with `tracing::Instrument::in_current_span()` so the caller's span follows the future across the `tokio::spawn` boundary. Any tracing layer the consumer installs sees background-task events as descendants of the originating request.
- `eprintln!` calls in the worker, matcher, and retry paths replaced with `tracing::warn!` / `error!` so subscribers actually see them. `tracing` becomes a direct dependency.

## 0.1.1 — 2026-04-19

- Pinned dependency versions for crates.io publishing.
