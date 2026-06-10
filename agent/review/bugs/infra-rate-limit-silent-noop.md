# HttpClientPool::with_rate_limit silently does nothing when pool was built without one

- **Severity:** low
- **Category:** bugs
- **Location:** `vantage-api-pool/src/client_pool/http.rs:109`

`with_rate_limit` only adjusts an *existing* `KeyedRateLimiter`; if the pool was constructed with `rate_limit: None`, the call is a silent no-op and the pool runs unthrottled despite the caller explicitly requesting a cap. Additionally, `RateLimiter::set_desired_rate` computes `Decimal::ONE / rate` (`rate_limit/rate_limiter.rs:21`), which panics on a zero rate, and `Decimal::from_usize(self.workers).unwrap()` divides by the worker count with no zero-guard.

```rust
/// Cap at rate_limit requests per second
pub fn with_rate_limit(mut self, rate_limit: Decimal) -> Self {
    let desired_rate = rate_limit / Decimal::from_usize(self.workers).unwrap();
    if let Some(rl) = self.rate_limit.as_mut() {
        rl.set_desired_rate(desired_rate);
    }
    self
}
```

**Recommendation:** Create the limiter when absent (it is already behind an `Option<Arc<_>>`), or return an error; validate `rate > 0` and `workers > 0` at construction.
