# AWS_ENDPOINT_URL override honoured only by json1/json10 transports

- **Severity:** medium
- **Category:** inconsistencies
- **Location:** `vantage-aws/src/restjson/transport.rs:30`

`AwsAccount` documents that the endpoint override (set via `with_endpoint` or `AWS_ENDPOINT_URL`, "picked up automatically by every constructor") redirects requests to DynamoDB Local/LocalStack. But only the JSON transports route through `account.endpoint_for(service)`. The REST-JSON, REST-XML (`restxml/transport.rs:166`), and Query (`query/transport.rs:39-42`) transports hard-code `{service}.{region}.amazonaws.com`, so with an endpoint override configured, Lambda/S3/IAM tables silently send real AWS requests — signed with whatever (possibly dummy LocalStack) credentials are loaded — instead of hitting the local endpoint.

```rust
// restjson/transport.rs
let host = format!("{service}.{region}.amazonaws.com");
let url = build_url(&host, path, query);
```

**Recommendation:** Route all transports through `endpoint_for()` (it already returns `(url, host)`), or return a hard error from the non-JSON transports when an endpoint override is configured so the mismatch is loud instead of silent.
