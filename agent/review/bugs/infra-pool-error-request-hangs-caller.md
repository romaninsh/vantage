# Failed pool requests are dropped, leaving callers awaiting forever

- **Severity:** high
- **Category:** bugs
- **Location:** `vantage-api-pool/src/client_pool/http.rs:101`

When a worker gets `EventualRequestResult::Error` (any non-success, non-429, non-5xx status — including 401/403/404 — or a missing/un-clonable request body), it only logs and drops the `EventualRequest`. The matching `oneshot::Sender` stays in `EventualRequestMatcher::pending_requests` forever, so the caller blocked on `receiver.await` in `matcher/mod.rs:76` (`AwwPool::request`/`get`) never resolves — every 4xx response turns into a permanent hang plus a map-entry leak.

```rust
EventualRequestResult::Error(e) => {
    error!(error = %e, worker = w, "http request failed in worker");
}
```

**Recommendation:** Route errored requests back through the response channel (the matcher already delivers the `EventualRequest`; `send()` can surface `Error` from it), or remove and drop the pending oneshot sender so `receiver.await` fails fast with `RecvError`.
