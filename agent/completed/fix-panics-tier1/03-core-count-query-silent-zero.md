# get_count_via_query returns Ok(0) for unrecognized result shapes

- **Severity:** medium
- **Category:** omissions
- **Location:** `vantage-table/src/table/impls/selectable.rs:209-221`

If the driver's count query returns anything other than `{"count": n}` or a bare integer (e.g. SurrealDB-style `[{"count": 42}]` arrays, a string-typed number, or an unexpected error payload), `get_count_via_query` silently reports `Ok(0)`. A wrong-but-successful zero is the worst failure mode for a count used in pagination or "is empty" checks — the table appears empty and the bug is invisible.

```rust
pub async fn get_count_via_query(&self) -> Result<i64> {
    let count_query = self.get_count_query();
    let result = self.data_source.execute(&count_query).await?;

    // Extract count from result - could be {"count": 42} or just 42
    if let Some(count) = result.get("count").and_then(|v| v.as_i64()) {
        Ok(count)
    } else if let Some(count) = result.as_i64() {
        Ok(count)
    } else {
        Ok(0)
    }
}
```

**Recommendation:** Return `Err(error!("unexpected count result shape", result = result))` in the fallback arm (and consider also handling the single-element-array shape explicitly).
