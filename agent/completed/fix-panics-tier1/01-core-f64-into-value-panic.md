# IntoValue for f64 panics on NaN/Infinity

- **Severity:** medium
- **Category:** bugs
- **Location:** `vantage-expressions/src/value.rs:31`

The `IntoValue` impl for `f64` unwraps `serde_json::Number::from_f64`, which returns `None` for `NaN`, `+Infinity` and `-Infinity`. Any value that flows from user data or a computation (e.g. a division by zero producing `inf`) into an expression parameter will panic the process instead of returning an error or `Value::Null`.

```rust
impl_into_value! {
    i32 => |v| Value::Number(serde_json::Number::from(v)),
    i64 => |v| Value::Number(serde_json::Number::from(v)),
    u64 => |v| Value::Number(serde_json::Number::from(v)),
    f64 => |v| Value::Number(serde_json::Number::from_f64(v).unwrap()),
    bool => Value::Bool,
    String => Value::String,
    ...
}
```

**Recommendation:** Map non-finite floats to `Value::Null` (`Number::from_f64(v).map(Value::Number).unwrap_or(Value::Null)`) or make `into_value` fallible; never `unwrap()` on data-derived floats.
