# Using the API adapters from business logic

This document is for Rust code that is not a UI: an API handler, a CLI, a
sync job, a library that fronts a partner's HTTP API. You want typed rows
out of a REST or GraphQL endpoint with the same `Table` you use for SQL,
and you want the call to behave sensibly when the endpoint is slow, rate
limited or down.

Short version: build one `RestApi` or `GraphqlApi` per remote API, clone it
into your tables, and decide per call whether anyone is waiting.

## The minimum useful client

```rust
use vantage_api_client::{ResponseShape, RestApi, eq_condition};
use vantage_table::table::Table;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct Order { id: String, customer: String, total: f64 }

let api = RestApi::builder("https://orders.example.com/v1")
    .response_shape(ResponseShape::Wrapped { array_key: "items".into() })
    .total_key("count")
    .auth(format!("Bearer {token}"))
    .build();

let orders = Table::<RestApi, Order>::new("orders", api.clone()).with_id_column("id");

let mut open = orders.clone();
open.add_condition(eq_condition("status", "open"));
for (id, order) in open.list().await? {
    println!("{id}: {} {}", order.customer, order.total);
}
```

`list()` returns typed entities, `list_values()` raw records, `get(id)` one
entity by id, `get_count()` the envelope total when the API has one.

## Decide who is waiting

Every request picks its retry policy from a task-local, not from the
table. The default is "nobody is waiting": one attempt, and if the API is
down the call fails at once.

```rust
use vantage_core::Priority;

// A request handler: the user is waiting, so retry until they hang up.
async fn get_orders(State(orders): State<Table<RestApi, Order>>) -> Result<Json<Vec<Order>>, AppError> {
    let rows = Priority::Essential
        .scope(orders.list())
        .await?;
    Ok(Json(rows.into_values().collect()))
}

// A sync job's poll: nothing is waiting, one attempt is right.
let rows = orders.list().await;
```

Cancellation is the only exit from an essential wait: when axum drops the
handler's future because the client disconnected, the retry loop stops with
it. Wrap an essential call in `tokio::time::timeout` if you want a ceiling.
A task you `tokio::spawn` does not inherit the priority; scope inside the
spawned future.

## Handle errors by kind

```rust
match orders.list().await {
    Ok(rows) => …,
    Err(e) => {
        let text = e.to_string();          // "API request failed (url: …, attempts: 1, kind: status, status: 503)"
        if text.contains("kind: breaker_open") {
            // the API is known to be down; serve stale data or a 503 of your own
        } else if text.contains("status: 4") {
            // the request was wrong; do not retry
        } else {
            // transport or 5xx after retries
        }
    }
}
```

The context keys are `url` (or `endpoint`), `attempts`, `kind` (`status`,
`transport`, `breaker_open`, `auth`), `status`, `body` (first 500 chars of
the server's response) or `detail`. If you need them structurally rather
than through `Display`, read `VantageError::context`.

## Know the API's state

```rust
use vantage_api_client::BreakerState;

match api.breaker_state() {
    Some(BreakerState::Open { until }) => tracing::warn!(?until, "orders API is down"),
    _ => {}
}
```

For continuous visibility attach an observer at build time:

```rust
use std::sync::Arc;
use vantage_api_client::{TransportEvent, TransportObserver};

struct Metrics;
impl TransportObserver for Metrics {
    fn on_event(&self, key: &str, event: TransportEvent) {
        if let TransportEvent::Failed { error, ms } = event {
            metrics::counter!("api_failures", "api" => key.to_string(), "kind" => error.kind_name()).increment(1);
        }
    }
}

let api = RestApi::builder(url).observer("orders", Arc::new(Metrics)).build();
```

The callback runs on the request path: record and return, never block.

## Writes

`Table` writes are refused on both adapters. For a REST write use the
API's own helper, which shares the cap, breaker and health:

```rust
use reqwest::Method;

let key = uuid::Uuid::now_v7().to_string();
let resp = api
    .http_request(Method::POST, "orders", &[("Idempotency-Key", &key)], Some(&payload))
    .await?;
```

If a write may be retried, generate the idempotency key once and reuse it
on every attempt; a server that stores its response per key makes the
retry harmless. For GraphQL, post the mutation document with
`api.post_graphql(doc, &variables)` under the same rules.

## Patterns by shape of work

**Request handler fronting a slow API.** Build the API once in app state.
Wrap reads in `Priority::Essential.scope(..)` and let the client's cap
(`max_parallel`) protect the upstream. Add a Diorama lens with a cache if
the same rows are read far more often than they change.

**Sync job.** Leave the default priority: one attempt per poll, and check
`breaker_state()` at the top of each cycle to skip a cycle cheaply while
the API is down. Report progress from `RowsPulled` if you attach an
observer.

**CLI.** Wrap the whole command in `Priority::Essential.scope(..)` with a
`timeout`, so a flaky API is retried but the user is never stuck. Print
`err.to_string()`: it carries the status and the server's message.

**Library over a partner API.** Expose your own `Table` factories over a
`RestApi` the caller builds, so the caller controls auth, the cap and the
observer. Never build a `RestApi` per table inside the library.

## Mixing backends

Both adapters bridge into `AnyTable` and into `Vista`, so an application
can hold a `Vec<AnyTable>` of SQL, REST and GraphQL tables or hand a REST
Vista to Diorama exactly like a SQLite one. `Vista::driver()` reports
`"rest-api"` or `"graphql"` for code that needs to branch on the backend.
See [README_rest.md](README_rest.md), [README_graphql.md](README_graphql.md)
and [README_transport.md](README_transport.md) for the details of each
layer.
