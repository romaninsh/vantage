# How requests are retried, throttled and reported

Every REST and GraphQL request this crate sends goes through one
`ResilientClient` per `RestApi` / `GraphqlApi` (from `vantage-api-pool`).
This document answers the questions that come up once an API misbehaves:
who retries, when, for how long, what stops it, and what the rest of the
process can learn about it. The mechanics inside the client are in
[vantage-api-pool's ARCHITECTURE.md](../vantage-api-pool/ARCHITECTURE.md);
this is the view from the datasource side.

## Who retries: the client, never the datasource

There is exactly one retry loop, inside `ResilientClient::execute_with`.
`RestApi::fetch_raw_body` and `GraphqlApi::post_graphql` call it once per
logical request and get back either a `2xx` response or a `ClientError`.
They never loop themselves, and neither does anything above them: a Diorama
poll that fails waits for its next tick, a viewport load that fails waits
for the next viewport event. Two nested loops would multiply attempts and
make back-off unpredictable, so there is one.

## Priority decides the policy

The datasource reads `vantage_core::Priority::current()` at the moment the
request is about to be sent and maps it:

| `Priority` | `CallPolicy` | Behaviour |
|---|---|---|
| `Background` (the default) | `background()` | one attempt; while the breaker is open, fail at once with `kind: breaker_open` |
| `Essential` | `essential()` | retry 250 ms doubling to 10 s until the awaiting future is dropped; while the breaker is open, sleep until the half-open probe slot and take it |

`Priority` is a tokio task-local. Whoever owns the wait sets it once:

```rust
use vantage_core::Priority;

// A user is looking at an empty grid: retry until they leave.
let rows = Priority::Essential.scope(table.list_values()).await?;

// A timer-driven refresh over rows we already show: one attempt.
let rows = table.list_values().await?;
```

Everything awaited inside the scope inherits it, however many crates down.
A task spawned with `tokio::spawn` does not inherit it; the spawner wraps
the spawned future in its own `scope`. Outside any scope, and outside any
runtime, `current()` is `Background`.

Only the owner of a wait should set the priority: in the Vantage app that is
the Diorama loader (an uncached viewport), the lens (a cold page open) and
the outbox (a write). Datasources and drivers only read it.

## What a status means

| Response | Retried | Breaker | What the datasource gets |
|---|---|---|---|
| `2xx` | — | closes if open, resets the failure run | rows |
| `5xx`, connection error, timeout | yes, per the policy | counts toward opening | after retries: error with `status`, `attempts`, `kind: status` or `transport` |
| `408`, `429` | yes, per the policy; `Retry-After` honoured up to the policy's ceiling | not counted, not closed | as above |
| `401` (auth configured) | replayed once with a fresh token | closes if open | rows, or a `401` error if the fresh token fails too |
| other `4xx` | no | closes if open | error on the first attempt, with `status` and up to 500 chars of the body |
| `200` with a GraphQL `errors` array | no | closed (it was a `2xx`) | error carrying the server's messages |

The last row matters for GraphQL: a logical error is detected after the
transport has already succeeded, so it can never retry and never counts as
a failure.

### The breaker

Five consecutive `5xx`/transport failures open the breaker for 5 s; each
failed probe doubles that, up to 60 s; the first answer of any kind closes
it. Background calls then fail without a request; essential calls wait for
the probe. `RestApi::breaker_state()` / `GraphqlApi::breaker_state()` report
`Closed`, `Open { until }` or `HalfOpen`.

### Rate limiting from the server's side

If an API answers `429` forever, nothing bad happens beyond the calls
failing: a `429` never opens the breaker (the server is answering), a
background call fails once per poll, an essential call keeps retrying at up
to 10 s spacing until cancelled. A `Retry-After` that is a positive whole
number of seconds is respected up to the policy's ceiling; an HTTP-date, a
`0`, a negative value or a date in the past is ignored and the policy's own
schedule applies. There is no way for a misconfigured server to make the
client spin.

### Stale cursors and wrong requests

This crate paginates by offset or page number, so there is no cursor to go
stale. If one is added, the rule above already gives the right answer for
a server that rejects it with a `4xx`: one attempt, final. A server that
answers a stale cursor with a `5xx` is indistinguishable from an outage and
an essential call would retry until cancelled; the driver must detect that
case and restart the listing rather than resend the request.

## Cancellation

Dropping the future returned by `list_values()`, `get()`, `fetch_window`
or `post_graphql` drops the in-flight request, any pending back-off sleep,
the semaphore permit and the breaker's probe slot. Nothing is spawned on
the request path, so nothing outlives the caller. This is how an essential
read stops: the grid scrolls on, the loader drops the old future, and the
retry loop ends.

## Per-datasource limits

| Builder method (both APIs) | Default | Meaning |
|---|---|---|
| `max_parallel(n)` | 4 | requests in flight to this API at most; the permit covers only the request, not the waits |
| `rate_limit(per_second)` | none | token bucket, burst `ceil(per_second)` |
| `observer(key, Arc<dyn TransportObserver>)` | none | report traffic under `key` (the datasource name) |
| `http_client(reqwest::Client)` (`client(..)` also on GraphQL) | default client | timeouts, proxies, TLS |

The breaker is always on with the defaults above.

## What an observer sees

The observer receives, per attempt, `Started` then exactly one of
`Succeeded { status, ms, bytes }` or `Failed { error, ms }`; per retry,
`RetryScheduled { after, attempt }`; per breaker change, `BreakerOpened {
cooldown }` (repeated on each failed probe with a longer cooldown, no
`BreakerClosed` in between) and `BreakerClosed`. The datasource adds
`RowsPulled { n }` after it has parsed a page, and `WritePushed` after a
write is acknowledged.

Facts a health registry may assume:

- `ms` is one attempt's round trip: it excludes the breaker wait, the token
  wait, the permit wait, the retry back-off and the auth round trip.
- A fail-fast rejection emits a synthetic `Started` + `Failed { ms: 0 }`
  with `error.kind_name() == "breaker_open"`; filter those out of request
  rates.
- The callback runs synchronously on the request path and must not block,
  await or call back into the datasource.

## Errors as the datasource reports them

A transport or status failure becomes a `VantageError` whose message is
`API request failed` (REST) or `GraphQL request failed` (GraphQL), with the
detail in the context, which `Display` prints as `key: value`:

| Context key | Present when | Value |
|---|---|---|
| `url` / `endpoint` | always | the request URL |
| `attempts` | always | attempts made, including the first |
| `kind` | always | `status`, `transport`, `breaker_open`, `auth` |
| `status` | the last attempt got a response | the HTTP status |
| `body` | the last attempt got a non-2xx with a body | first 500 chars |
| `detail` | no response | the transport error text |

So `err.to_string().contains("503")` works, and so does reading the
server's own message from a `422`.

## Writes

`RestApi::http_request(method, path, headers, body)` sends any method
through the same client: it joins `base_url` and `path`, applies the
configured auth, then the caller's headers (a caller header of the same name
replaces the configured one), a JSON body when given, chooses the policy
from `Priority::current()`, and reports `WritePushed` on a non-`GET`
success. A caller that needs the raw client, for example to share its cap
and breaker with something this helper does not cover, has
`RestApi::client()` and `GraphqlApi::client()`.

Retrying a write is only safe if the server treats a repeat as a no-op.
Send an `Idempotency-Key` header with a value that stays fixed for the
lifetime of the logical write, and have the server replay its stored
response for a repeated key. The Vantage app's outbox does exactly that.
