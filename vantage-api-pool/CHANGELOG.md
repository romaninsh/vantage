# Changelog

## 0.6.1 — unreleased

- `ResilientClient` — async-native HTTP transport that replaces the worker-pool
  + oneshot-matcher model (`AwwPool` / `HttpClientPool` / `EventualRequest`).
  Concurrency is bounded by a semaphore (per-API parallelism cap), and the
  genuinely-valuable policies are inline middleware: retry with exponential
  backoff + jitter on 429/5xx/network (honoring `Retry-After`, bounded by
  `max_retries`), auth-token refresh on 401 (re-acquire + replay), and a circuit
  breaker (fail fast after N consecutive failures, half-open probe after a
  cooldown). Cancellation is structural — drop the future, drop the request.
  Covered by wiremock contract tests. The old `AwwPool` path is unchanged and
  will be retired once consumers (the Vantage REST datasource, the desktop app)
  migrate onto `ResilientClient`.
- `ResilientClient::execute_with(&CallPolicy, ..)`: per-call retry mode
  (`None`, `Bounded`, `UntilCancelled`) and breaker mode (`FailFast`,
  `WaitForProbe`). `execute` is unchanged.
- Breaker cooldown doubles after each failed probe (`circuit_breaker_growing`,
  `default_breaker`); a success resets it.
- `ClientError { kind, attempts, body }` replaces `anyhow::Error` from
  `execute`; `body` is the first 2 KiB of the failing response.
- `TransportObserver` hook: per-attempt start/success/failure, retries and
  breaker transitions, plus caller-reported `RowsPulled` / `WritePushed`.
  `on_event` runs on the request path: it must not block, await or re-enter
  the client. `BreakerOpened` repeats on every failed probe.
- `rate_limit(per_second)` token bucket per client.
- `breaker_state() -> Option<BreakerState>` and `in_flight()` for consumers
  that poll instead of following events.
- A semaphore permit covers only the request, not breaker cooldowns or retry
  back-offs, so a fail-fast caller is never queued behind a waiting one.
- Breaker failures are 5xx and transport errors only. A 4xx closes an open
  breaker without clearing the failure run; 408 and 429 leave it untouched.
- `Retry-After` is clamped to the call policy's back-off ceiling.

## 0.6.0 — unreleased

- Coordinated 0.6 release; internal dependencies realigned to 0.6. No public API changes.

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
