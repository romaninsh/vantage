# vantage-api-pool Architecture

`ResilientClient` is one HTTP client per remote API. It owns everything that
should be decided per API rather than per request: how many requests may be
in flight, how fast they may leave, when to stop trying because the API is
down, how a failed attempt is retried, and what the rest of the process gets
told about all of it. Callers hand it a request builder and a policy and
`.await` a response; there are no worker threads, no queues and no
callbacks other than the observer.

This document is for maintainers and for authors of consumers that need to
know exactly what the client promises. The user-facing guide is the
[README](README.md).

## Layer diagram

```
caller (RestApi, GraphqlApi, an outbox, a test)
   │  execute_with(&CallPolicy, |http| http.get(url))
   ▼
ResilientClient                     mod.rs, builder.rs
   │  one per API: semaphore, rate bucket, breaker, auth, observer
   ▼
AttemptLoop::run                    attempt.rs
   │  loop { wait_for_gate → send_once → record }
   ├── CircuitBreaker               breaker.rs
   ├── TokenBucket                  rate.rs
   ├── AuthState                    auth.rs
   ├── CallPolicy / RetryMode       policy.rs
   ├── ClientError / ErrorKind      error.rs
   └── TransportObserver events     observer.rs
   ▼
reqwest::Client (shared, cloneable)
```

Everything above `reqwest` is plain `async` on whatever runtime the caller
is on. The crate spawns nothing. Dropping the future returned by
`execute_with` drops the in-flight request, any pending back-off sleep, the
semaphore permit and the probe slot with it.

## One call, attempt by attempt

`execute_with(policy, build)` runs a loop. Each iteration is one attempt and
goes through three named steps, in this order and with these guarantees:

1. **`wait_for_gate`** — holds no permit.
   - Ask the breaker. `Gate::Allow { probe }` lets the attempt run;
     `probe: Some(epoch)` means this attempt is the half-open probe and a
     `ProbeGuard` is created for it. `Gate::OpenFor(wait)` means the breaker
     is open: under `BreakerMode::FailFast` the attempt is rejected at once
     with `ErrorKind::BreakerOpen`; under `WaitForProbe` the loop sleeps
     `wait` and asks again, until it is granted the probe.
   - Take a rate-limit token, sleeping until one is available.
2. **`send_once`** — acquires the semaphore permit and the in-flight counter,
   and holds both only until the response head has arrived.
   - Report `Started`.
   - Call `build(&http)` for a fresh `RequestBuilder`, apply the auth header
     (fetching a token lazily on first use).
   - Send. `ms` is measured from just before `send` to the response head; it
     excludes the auth round trip, the gate wait, the token wait and the
     permit wait.
   - Classify: `2xx` is `Success`; a `401` on a client with an auth
     refresher, not yet refreshed in this call, is `Unauthorized`; anything
     else is `Failed { kind, body, hint }`, where `body` is the first 2 KB of
     the response and `hint` the `Retry-After` header in whole seconds.
3. **`record`** — permit already released.
   - Tell the breaker what the attempt proved (see the health table below),
     release the probe slot, then report the terminal event and any breaker
     transition, in that order.
   - Decide: `Return` the response or error; `Replay` at once after a token
     refresh; or `Retry(after)` where `after` is the policy's back-off, or
     the server's `Retry-After` clamped to the policy's ceiling, plus up to
     25% jitter. `RetryScheduled` is reported before the sleep.

Because the permit is released before any sleep, a caller parked in a
cooldown or a back-off never occupies the API's parallelism budget. A
`FailFast` caller gets its answer without queueing behind one that is
waiting.

## What counts as what

The breaker measures **reachability**, not correctness. A response that a
retry cannot fix still proves the API is up.

| Attempt outcome | Retryable | Breaker | Notes |
|---|---|---|---|
| `2xx` | — | `record_success`: closes if open, resets the cooldown and the failure run | |
| `5xx`, transport error | yes | `record_failure`: counts toward the threshold; a failed probe doubles the cooldown | |
| `408`, `429` | yes | untouched (`Inconclusive`) | retried with the policy's back-off; a `Retry-After` is honoured up to the ceiling |
| `401` with a refresher, first time in the call | replay | `record_reachable`: closes if open, keeps the failure run | token re-acquired, attempt replayed; no retry budget spent |
| any other `4xx`, or a second `401` | no | `record_reachable` | final: returned to the caller after one attempt |
| auth refresher error | no | untouched | `ErrorKind::Auth`, returned at once |
| breaker open, `FailFast` | no | untouched | `ErrorKind::BreakerOpen`, no request sent |

`record_reachable` differs from `record_success` in one way: it does not
zero `consecutive_failures`. A stream of `503, 404, 503, 404, 503` still
opens the breaker on the third `503`.

## Retry policy

`CallPolicy { retry: RetryMode, breaker: BreakerMode }`.

- `RetryMode::None` — one attempt. `CallPolicy::background()` pairs it with
  `FailFast`. For work nobody is waiting on: polls, refreshes, hydration.
- `RetryMode::UntilCancelled { base, max }` — retry forever, exponential from
  `base` capped at `max`. `CallPolicy::essential()` uses 250 ms → 10 s with
  `WaitForProbe`. The only exit is the caller dropping the future.
- `RetryMode::Bounded(RetryPolicy)` — the classic `max_retries` schedule.
  `execute` is `execute_with(&CallPolicy::bounded(client_default))`.

Back-off is never shorter than 1 ms even if a policy is built with a zero
base, so a misconfigured policy cannot spin. `Retry-After` is trusted only
as a positive whole number of seconds; HTTP-date forms, `0` and negative
values are ignored in favour of the policy's own schedule.

## Breaker state machine

```
          threshold consecutive failures
 Closed ─────────────────────────────────▶ Open(cooldown)
   ▲                                          │ cooldown elapses
   │ probe 2xx / reachable                    ▼
   │ (cooldown → base)                     HalfOpen ── one caller granted the probe
   │                                          │
   └──────────────────────────────────────────┤ probe 5xx / transport
                                              ▼
                                        Open(min(2 × cooldown, max))
```

Defaults from `default_breaker()`: threshold 5, cooldown 5 s doubling to
60 s. While a probe is in flight every other caller sees `OpenFor(5 ms)` and
re-checks.

**Probe epochs.** Every grant of the probe slot carries a fresh epoch. The
`ProbeGuard` releases the slot on drop only if its epoch is still the
current one, and a failure only doubles the cooldown if it was reported
with the current probe's epoch. So a cancelled prober cannot wedge the
breaker open, a stale guard cannot free a newer grant's slot, and a failure
from a request that started before the breaker opened cannot be mistaken
for a failed probe.

`breaker_state()` reports `Closed`, `Open { until }` or `HalfOpen` for
consumers that poll instead of tracking events.

## Rate limit

`rate_limit(per_second)` is a token bucket: `per_second` tokens refill
continuously up to a burst of `ceil(per_second)` (or the explicit burst from
`rate_limit_with_burst`). Each attempt takes one token before acquiring a
permit, so the wait is neither reported nor counted in `ms`. A token spent
by a caller that then waits for a permit is not refunded; the long-run rate
holds, a short burst after permits free up is possible.

## Observer contract

`TransportObserver::on_event(key, event)` is called synchronously on the
call's own task. The `key` is whatever the builder was given, which the
desktop app makes the datasource name; the crate has no idea what a
datasource is.

What a consumer may rely on:

- Every `Started` is followed by exactly one `Succeeded` or `Failed` for the
  same attempt, on every path, unless the caller's future is cancelled
  mid-send. A fail-fast rejection and a failed token refresh each emit a
  synthetic `Started` + `Failed { ms: 0 }`; their `error.kind_name()` is
  `breaker_open` or `auth`, so they can be filtered out of request-rate
  figures.
- The last `Failed` of a call carries the same `ClientError` the call
  returns.
- `RetryScheduled { after, attempt }` precedes every retry sleep; a 401
  replay reports none.
- `BreakerOpened { cooldown }` fires when the breaker opens and again on
  every failed probe, with a longer cooldown each time and no intervening
  `BreakerClosed`. Set the deadline; do not count the events.
  `BreakerClosed` fires once when a success or a reachable answer closes
  it.
- `RowsPulled { n }` and `WritePushed` are never emitted by the client
  itself; callers report them through `ResilientClient::report` once they
  have parsed a response or seen a write acknowledged.
- The probe slot is released before any callback runs, and no lock is held
  during a callback.

What a consumer must do: return quickly, never block or `.await`, never
call back into the client. The `Started` callback runs while the attempt's
permit is held.

## Errors

`ClientError { kind, attempts, body }`:

- `kind`: `Status(u16)`, `Transport(String)`, `BreakerOpen`, `Auth(String)`,
  `Closed` (reserved for an explicit client shutdown; nothing produces it
  today).
- `attempts`: how many attempts were made, including the first.
- `body`: up to 2048 bytes of the last non-2xx response, cut on a char
  boundary; `None` for every other kind.

`status()` and `kind_name()` give the two fields consumers switch on.
`Display` is `HTTP 503 after 3 attempts`; it never includes the URL or the
body.

## Concurrency and cancellation

- The semaphore caps in-flight requests per client (`max_parallel`,
  default 8 here; the API datasource layer defaults to 4). It is held only
  inside `send_once`.
- `in_flight()` / `peak_in_flight()` are maintained by an RAII guard, so a
  cancelled call decrements them.
- The breaker and the token bucket lock a `std::sync::Mutex` only inside
  synchronous methods that return before any `.await`.
- `AuthState` caches the token behind a `tokio::sync::RwLock`; a `401`
  triggers one re-acquire per call, single-flight is not attempted.

## What the crate deliberately does not do

- It does not decide what a stale pagination cursor or a wrong filter means.
  Those come back as final `4xx` errors on the first attempt; recovering
  (re-listing from page one, dropping a condition) is the caller's job.
- It does not spawn or detach work. If a consumer needs a request to
  outlive the caller, the consumer spawns and owns that task.
- It does not persist anything. A queue of writes that must survive a
  restart lives above this crate.

## Legacy

`AwwPool`, `HttpClientPool`, `EventualRequest`, `EventualRequestMatcher`,
the four paginated streams and the `rate_limit` module predate
`ResilientClient`. They use worker threads and a oneshot matcher, have known
defects (a `4xx` leaves the caller waiting forever; the auth token is never
refreshed), and have no consumers in the workspace. They will be removed
once the remaining examples migrate; nothing new should build on them.
