# Plaintext credentials exposed through derived Debug on connection/auth types

- **Severity:** medium
- **Category:** security
- **Location:** `surreal-client/src/connection.rs:31`

`AuthParams` derives `Debug` and holds `username`/`password`/JWT `Token` as plain `String`s; `SurrealConnection` also derives `Debug` and embeds an `Option<AuthParams>`. Any `{:?}` formatting of a connection or auth value — a `tracing` span field, an `.expect()`/`anyhow` context, a panic message — prints the password/token in clear text. Credential-bearing structs should never carry a naive derived `Debug`.

```rust
#[derive(Debug, Clone)]
pub enum AuthParams {
    Root { username: String, password: String },
    // …
    Token(String),
}
#[derive(Default, Debug, Clone)]
pub struct SurrealConnection { /* … */ auth: Option<AuthParams>, /* … */ }
```

**Recommendation:** Hand-implement `Debug` to redact secret fields (`password: "***"`), or wrap secrets in a `Secret<String>` newtype whose `Debug` masks the value.
