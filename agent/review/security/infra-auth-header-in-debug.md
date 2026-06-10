# API auth tokens exposed via derived Debug on RestApi/GraphqlApi

- **Severity:** high
- **Category:** security
- **Location:** `vantage-api-client/src/rest/api.rs:103`

`RestApi` (and `GraphqlApi` at `src/graphql/api.rs:24`) derive `Debug` while holding the raw `Authorization` header value (e.g. `Bearer <token>`). Any `{:?}` log line, error context, or panic message that includes the datasource prints the credential in plaintext. This is inconsistent with `AwsAccount` in vantage-aws, which hand-implements `Debug` and redacts `access_key`/`secret_key`/`session_token`.

```rust
#[derive(Clone, Debug)]
pub struct RestApi {
    base_url: String,
    client: reqwest::Client,
    pub(crate) auth_header: Option<String>,
    ...
}
```

**Recommendation:** Implement `Debug` manually for `RestApi`, `RestApiBuilder`, `GraphqlApi`, and `GraphqlApiBuilder`, printing `auth_header: Some("<redacted>")`, following the existing `AwsAccount` pattern.
