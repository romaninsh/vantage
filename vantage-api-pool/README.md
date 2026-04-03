# vantage-api-pool

High-performance REST API client pool for the [Vantage](https://github.com/romaninsh/vantage) data
framework.

Provides `PoolApi`, a `TableSource` implementation backed by a concurrent HTTP client pool with
automatic pagination, prefetching, and rate limiting. Use the same `Table<PoolApi, E>` / entity
pattern as any other Vantage backend.

## Quick start

```rust
use vantage_api_pool::{AwwPool, PoolApi};

// Create pool with 3 workers and Bearer auth
let pool = Arc::new(
    AwwPool::new(3, None, false, "http://api.example.com".to_string())
        .with_auth_callback(1,
            || async { Ok(fetch_token().await) },
            |mut req, token| {
                req.headers_mut().insert("Authorization",
                    format!("Bearer {}", token).parse().unwrap());
                req
            },
        ),
);

// Wrap as a Vantage TableSource
let api = PoolApi::new(pool);

// Use with Table and entities — auto-paginates transparently
let countries = Table::new("countries", api.clone()).with_id_column("name");
let all = countries.list_values().await?;  // fetches all pages

// Entity access
let germany: Country = countries.get("Germany").await?;
```

## Architecture

```
PoolApi (TableSource)
  └── AwwPool
        ├── HttpClientPool (N worker threads)
        │     └── EventualRequest (retry, backoff, rate limiting)
        ├── EventualRequestMatcher (request/response routing)
        └── PaginatedStream (async Stream with prefetch)
```

**Request flow:** `Table.list_values()` -> `PoolApi.list_table_values()` -> `PaginatedStream`
fetches pages concurrently via `AwwPool.get()` -> worker threads execute HTTP requests -> responses
are matched back and yielded as stream items.

## Modules

### Core pool

| File                          | Description                                                                                                                            |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `src/aww_pool.rs`             | `AwwPool` — top-level pool API. Manages workers, auth tokens, and request dispatch.                                                    |
| `src/client_pool/http.rs`     | `HttpClientPool` — spawns N worker threads that process HTTP requests from a channel. Handles per-worker rate limiting.                |
| `src/eventual_request/mod.rs` | `EventualRequest` — wraps a single HTTP request with retry logic. Exponential backoff on 429/5xx errors.                               |
| `src/matcher/mod.rs`          | `EventualRequestMatcher` — routes responses back to callers using ID-based matching. Async coordination between senders and receivers. |

### Pagination

| File                                 | Description                                                                                                                                                                                    |
| ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/paginator/paginated_stream.rs`  | `PaginatedStream` — implements `futures::Stream`. Fetches pages via tokio tasks with configurable prefetch depth. Expects `{"data": [...], "pagination": {"total_pages": N}}` response format. |
| `src/paginator/paginated_stream2.rs` | `PaginatedStream2` — alternative with sequential fetch + overlap.                                                                                                                              |
| `src/paginator/paginated_stream3.rs` | `PaginatedStream3` — pure sequential, minimal overhead.                                                                                                                                        |
| `src/paginator/paginated_stream4.rs` | `ItemStream4` — channel-based with dedicated worker thread.                                                                                                                                    |

`PaginatedStream` (used by `PoolApi`) is the best performer across all configurations.

### Rate limiting

| File                                   | Description                                                                              |
| -------------------------------------- | ---------------------------------------------------------------------------------------- |
| `src/rate_limit/keyed_rate_limiter.rs` | `KeyedRateLimiter` — per-key rate limiting with sleep-based throttling.                  |
| `src/rate_limit/policy.rs`             | `RateLimitPolicyEnforcer` — IETF-compliant rate limit policy (returns 429 with headers). |
| `src/rate_limit/damper.rs`             | Reserved for future adaptive dampening support; currently not implemented.               |
| `src/rate_limit/rate_limiter.rs`       | `RateLimiter` — simple single-key rate limiter.                                          |

### Statistics

| File                    | Description                                                 |
| ----------------------- | ----------------------------------------------------------- |
| `src/stats/mod.rs`      | `Stats` — tracks success/error/retry counts with timing.    |
| `src/stats/average.rs`  | `Average` — running average computation for response times. |
| `src/stats/interval.rs` | Interval-based stat differencing for live reporting.        |

### Vantage integration

| File              | Description                                                                                                                                                                                                 |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/pool_api.rs` | `PoolApi` — implements `TableSource` for `AwwPool`. `list_table_values()` auto-paginates by collecting a `PaginatedStream`. `stream_table_values()` exposes the stream directly for incremental processing. |

## AwwPool configuration

```rust
AwwPool::new(
    workers: usize,         // Number of HTTP worker threads (default: 3)
    rate_limit: Option<Decimal>,  // Requests/second per worker (None = unlimited)
    use_dampener: bool,     // Adaptive rate dampening on 429 responses
    base_url: String,       // API base URL
)
```

### Authentication

```rust
pool.with_auth_callback(
    n,                    // Number of auth tokens to cache
    || async { ... },     // Token acquisition function
    |req, token| { ... }, // Request modifier (add headers)
)
```

Tokens are acquired lazily and cached. Multiple tokens can be rotated for APIs with per-token rate
limits.

### Direct pool usage

```rust
// Simple GET (auth applied automatically)
let response = pool.get("/countries?page=1").await?;

// Custom request
let request = Client::new().post(&url).json(&body).build()?;
let response = pool.request(request).await?;
```

### Paginated streaming

```rust
use tokio_stream::StreamExt;

let mut stream = PaginatedStream::get(pool.clone(), "/countries".to_string())
    .prefetch(3);  // Keep 3 pages fetched ahead

while let Some(item) = stream.next().await {
    let value: serde_json::Value = item?;
    // Process each item as it arrives
}
```

## Vantage integration

`PoolApi` wraps `Arc<AwwPool>` and implements `TableSource`, so it works with `Table`, `Entity`,
`print_table`, and all other Vantage features.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct City {
    name: String,
    population: i64,
}

impl City {
    fn for_country(country: &str) -> Table<PoolApi, City> {
        let endpoint = format!("countries/{}/cities", urlencoding::encode(country));
        Table::new(&endpoint, pool())
            .with_id_column("name")
            .with_column_of::<i64>("population")
    }
}

// Auto-paginates, returns all cities across all pages
let cities = City::for_country("Argentina");
print_table(&cities).await?;

// Entity access
let rosario: City = cities.get("Rosario").await?;
println!("Population: {}", rosario.population);
```

## Expected API response format

```json
{
  "data": [
    { "name": "Berlin", "population": 3426354 },
    { "name": "Munich", "population": 1510378 }
  ],
  "pagination": {
    "page": 1,
    "per_page": 10,
    "total": 500,
    "total_pages": 50,
    "has_next": true,
    "has_prev": false
  }
}
```

Pagination is driven by `total_pages` from the first response. Pages are fetched as `?page=N` query
parameters.

## License

MIT OR Apache-2.0
