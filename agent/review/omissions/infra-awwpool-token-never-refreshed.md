# AwwPool caches the first auth token forever — no expiry or refresh

- **Severity:** medium
- **Category:** omissions
- **Location:** `vantage-api-pool/src/aww_pool.rs:110`

`get_auth_token` acquires a token once via the configured callback and caches it in `auth_tokens`; nothing ever removes or refreshes it. When the token expires, every subsequent request gets a 401/403, which the worker classifies as `EventualRequestResult::Error` — it is neither retried with a fresh token nor surfaced to the caller (see the dropped-request hang). A long-running Vantage UI session with OAuth-style short-lived tokens goes permanently stale after the first expiry.

```rust
// Check if we already have a cached token
{
    let tokens = self.auth_tokens.lock().unwrap();
    if !tokens.is_empty() {
        return Ok(tokens[0].clone());
    }
}
// No cached token, acquire a new one
let token = acquire_fn().await?;
```

**Recommendation:** Invalidate the cached token on 401/403 responses and re-run the acquire callback (with single-flight to avoid a stampede), or store an expiry alongside the token and refresh proactively.
