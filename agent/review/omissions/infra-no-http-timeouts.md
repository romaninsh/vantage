# No timeouts on any HTTP client (RestApi, GraphqlApi, AwsAccount, pool workers)

- **Severity:** medium
- **Category:** omissions
- **Location:** `vantage-api-client/src/rest/api.rs:536`

Every HTTP client in the API/infra crates is built with `reqwest::Client::new()`, which has no total-request timeout (only a connect-phase default). Affected: `RestApi` (`rest/api.rs:536`), `AwsAccount` (`vantage-aws/src/account.rs:53,77,121,138`), and the pool workers (`vantage-api-pool/src/client_pool/http.rs:37`). A stalled endpoint (or a slow-loris AWS-endpoint override) hangs the table read indefinitely; in Vantage UI that means a grid that never resolves, and in the pool it permanently occupies a worker.

```rust
pub fn build(self) -> RestApi {
    RestApi {
        base_url: self.base_url,
        client: reqwest::Client::new(),
        ...
    }
}
```

**Recommendation:** Build clients with `reqwest::Client::builder().timeout(...).connect_timeout(...)` (e.g. 30s/10s), optionally overridable per datasource.
