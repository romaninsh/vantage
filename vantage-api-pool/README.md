# vantage-api-pool

A resilient HTTP transport for the [Vantage](https://github.com/romaninsh/vantage)
data framework. `ResilientClient` is one client per remote API that decides,
once, how that API is talked to: how many requests may be in flight, how
fast they may leave, when to stop trying because the API is down, how a
failed attempt is retried, and what the rest of the process gets told.

It is what `vantage-api-client`'s REST and GraphQL datasources send through,
and what the Vantage desktop app builds its per-datasource health indicator
on. It is plain `async`: no worker threads, nothing spawned, and dropping
the future cancels the request.

## Quick start

```rust
use std::sync::Arc;
use vantage_api_pool::{CallPolicy, ResilientClient, TransportObserver};

let client = ResilientClient::builder()
    .max_parallel(4)                 // requests in flight to this API, at most
    .rate_limit(10.0)                // requests per second, at most
    .default_breaker()               // 5 failures open it for 5 s, doubling to 60 s
    .observer("orders-api", health)  // Arc<dyn TransportObserver>, keyed by API
    .build();

// Someone is waiting for this: retry until the future is dropped.
let resp = client
    .execute_with(&CallPolicy::essential(), |http| http.get(url))
    .await?;

// Nobody is waiting: one attempt, fail fast while the breaker is open.
let resp = client
    .execute_with(&CallPolicy::background(), |http| http.get(url))
    .await?;

// The classic bounded retry, using the client's default policy.
let resp = client.execute(|http| http.get(url)).await?;
```

The closure is called once per attempt with the shared `reqwest::Client`,
so every retry gets a fresh `RequestBuilder`. Auth, when configured, is
applied by the client on every attempt and re-acquired once on a `401`.

## The rules in one screen

- **A `4xx` other than 408 and 429 is final** under every policy: one
  attempt, then the error, with the status and up to 2 KB of the body.
- **5xx, 408, 429 and transport errors are retryable.** Only 5xx and
  transport errors count toward the breaker. `Retry-After` is honoured when
  it is a positive whole number of seconds, capped at the policy's ceiling.
- **Any answer closes an open breaker**, including a `4xx` or a `401`: the
  breaker measures whether the API answers, not whether it liked the
  request. A `2xx` also resets the failure count; a `4xx` does not.
- **The permit covers only the request.** A caller waiting out a cooldown
  or a back-off is not occupying the parallelism budget, so a fail-fast
  caller never queues behind it.
- **Cancellation is structural.** Drop the future and the request, its
  back-off and its probe slot go with it.

The full state machine, the observer contract and the reasons behind each
rule are in [ARCHITECTURE.md](ARCHITECTURE.md).

## Policies

| `CallPolicy` | Retry | Breaker open | Use for |
|---|---|---|---|
| `background()` | one attempt | fail fast with `ErrorKind::BreakerOpen` | polls, refreshes, hydration, anything nobody waits on |
| `essential()` | 250 ms doubling to 10 s, until the future is dropped | wait for the half-open probe and take it | a cold page, a viewport the user is looking at, a write |
| `bounded(RetryPolicy)` | `max_retries` with exponential back-off | fail fast | what `execute` does |

## Observing traffic

`observer(key, Arc<dyn TransportObserver>)` reports every attempt as
`Started` then `Succeeded { status, ms, bytes }` or `Failed { error, ms }`,
every retry as `RetryScheduled`, and every breaker transition as
`BreakerOpened { cooldown }` / `BreakerClosed`. Callers add `RowsPulled { n }`
and `WritePushed` through `client.report(..)` once they know. `ms` is one
attempt's round trip and excludes every wait. The callback runs on the
request path: return quickly, never block, never call the client.

`breaker_state()` answers `Closed`, `Open { until }` or `HalfOpen` for
anything that would rather poll.

## Errors

`ClientError { kind, attempts, body }` with `ErrorKind::{Status(u16),
Transport(String), BreakerOpen, Auth(String), Closed}`, `status()` and
`kind_name()`. `Display` is `HTTP 503 after 3 attempts`; the URL and the
body are never in the message.

## Legacy

`AwwPool`, `HttpClientPool`, `EventualRequest`, the paginated streams and
`PoolApi` are the previous design: worker threads, a oneshot matcher, and a
`TableSource` over a `{ "data": [...], "pagination": {...} }` envelope. They
still compile and ship, have no consumers in the workspace, and carry known
defects (a `4xx` hangs the caller; the auth token is never refreshed). They
are slated for removal; build new work on `ResilientClient`.

## License

MIT OR Apache-2.0
