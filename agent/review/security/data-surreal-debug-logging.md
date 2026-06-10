# DebugEngine prints all RPC params and responses (incl. secrets) to stdout

- **Severity:** high
- **Category:** security
- **Location:** `surreal-client/src/engines/debug.rs:21`

When debug mode is enabled (`SurrealConnection::with_debug(true)`), `DebugEngine` wraps the transport and `println!`s every RPC method, its full params, and the full response to stdout. This includes any `signin`/`authenticate` calls (username + password), `let_var` values, and entire result sets — i.e. raw user-database rows. For a product positioned as "local-first / private" that connects to user databases, dumping credentials and PII to stdout (which commonly lands in shell history, CI logs, journald) is a meaningful exposure.

```rust
fn log_request(&self, method: &str, params: &Value) {
    let params_str = serde_json::to_string(params).unwrap_or_default();
    println!("🔍 Surreal RPC: {} {}", method, params_str);  // logs signin {user,pass}
}
async fn send_message_cbor(&mut self, method: &str, params: CborValue) -> Result<CborValue> {
    println!("🔍 Surreal CBOR RPC: {} {:?}", method, params);
```

**Recommendation:** Redact auth methods (`signin`/`signup`/`authenticate`) and truncate/redact result bodies; route through `tracing` at `trace` level rather than unconditional `println!`.
