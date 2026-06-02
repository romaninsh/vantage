# Changelog

## 0.5.3 — 2026-06-02

- Track `vantage-table`'s new `TableSource::Source` associated type (set to `String`; no
  user-visible change).

## 0.5.2 — 2026-05-23

- Align all internal dependency versions to 0.5+. No public API changes.

## 0.5.0 — 2026-05-23

- Bumped to the 0.5 line to track
  [vantage-table 0.5.0](https://docs.rs/vantage-table/0.5.0/vantage_table/)'s opening of the
  `AnyTable` decommission cycle, aligning with the rest of the workspace. No code changes beyond the
  dependency pin.

## 0.1.4 — 2026-05-16

- Internal dependency version refresh; no public API changes.

## 0.1.3 — 2026-05-09

- Pins `vantage-types` to `>= 0.4.2`.

## 0.1.2 — 2026-05-03

- HTTP worker pool, request/response matcher, and all paginator variants now wrap their spawned
  futures with `tracing::Instrument::in_current_span()` so the caller's span follows the future
  across the `tokio::spawn` boundary. Any tracing layer the consumer installs sees background-task
  events as descendants of the originating request.
- `eprintln!` calls in the worker, matcher, and retry paths replaced with `tracing::warn!` /
  `error!` so subscribers actually see them. `tracing` becomes a direct dependency.

## 0.1.1 — 2026-04-19

- Pinned dependency versions for crates.io publishing.
