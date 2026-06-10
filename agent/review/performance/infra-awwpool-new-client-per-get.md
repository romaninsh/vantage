# AwwPool::get builds a fresh reqwest::Client per call

- **Severity:** low
- **Category:** performance
- **Location:** `vantage-api-pool/src/aww_pool.rs:75`

Every `AwwPool::get` constructs a brand-new `reqwest::Client` (connection pool, TLS config, resolver state) just to build a `Request` object; the request is then executed by the workers' own clients. Client construction is one of the more expensive reqwest operations and is explicitly recommended to be done once and reused. On a pool meant for high-volume paginated streaming (`PaginatedStream` calls `get` per page), this is per-page overhead for no benefit.

```rust
pub async fn get(&self, path: &str) -> anyhow::Result<Response> {
    let full_url = if path.starts_with('/') {
        format!("{}{}", self.base_url, path)
    } else {
        format!("{}/{}", self.base_url, path)
    };
    let mut request = Client::builder().build()?.get(&full_url).build()?;
```

**Recommendation:** Store one `reqwest::Client` on `AwwPool` (or use `Request::new(Method::GET, url.parse()?)`) instead of building a client per request.
