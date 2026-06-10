# Unbounded retries and uncapped server-controlled Retry-After sleep

- **Severity:** medium
- **Category:** bugs
- **Location:** `vantage-api-pool/src/eventual_request/mod.rs:117`

`EventualRequest::execute` returns `Retry` for every 429, 5xx, and network error, and the worker loop (`client_pool/http.rs:97`) re-runs the same request with no maximum attempt count — a permanently failing endpoint pins a worker forever and the caller never gets an answer. Additionally `extract_retry_delay` honours the server's `Retry-After` header with no upper bound, so a hostile or misconfigured server can park a worker with `retry-after: 99999999` (the sleep happens inside `execute`, occupying the worker for the whole duration).

```rust
Ok(response) if response.status() == 429 => {
    self.retries += 1;
    let delay = self
        .extract_retry_delay(&response)        // uncapped, server-controlled
        .unwrap_or_else(|| self.calculate_backoff_delay());
    ...
    sleep(delay).await;
    EventualRequestResult::Retry
}
```

**Recommendation:** Add a max-retries cap (e.g. 5–10, after which return `Error`), and clamp `Retry-After` to a sane ceiling (e.g. 60s).
