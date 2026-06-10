# IntoRecord blanket impls panic on entity serialization failure

- **Severity:** medium
- **Category:** bugs
- **Location:** `vantage-types/src/record.rs:237` (also `record.rs:281`)

The blanket `IntoRecord<serde_json::Value>` and `IntoRecord<ciborium::Value>` impls — the path every serde-derived entity takes on `insert`/`replace`/`patch` — use `.expect()` on serialization. Serialization of user-defined entities can legitimately fail (e.g. a `HashMap` with non-string keys for JSON, or a custom `Serialize` impl returning an error), turning a recoverable error into a process abort deep inside `Table::insert`. The trait itself is infallible (`fn into_record(self) -> Record<T>`), so implementations have no error channel.

```rust
impl<T> IntoRecord<serde_json::Value> for T
where
    T: serde::Serialize,
{
    fn into_record(self) -> Record<serde_json::Value> {
        let json_value = serde_json::to_value(self).expect("Failed to serialize to JSON");
        ...
    }
}
// CBOR twin:
let cbor_value = ciborium::Value::serialized(&self).expect("Failed to serialize entity to CBOR");
```

**Recommendation:** Make the conversion fallible (`TryIntoRecord` / `fn into_record(self) -> Result<Record<T>>`) mirroring `TryFromRecord`, or document the panic contract prominently. Note the CBOR impl additionally drops map entries with non-text keys silently (`filter_map`), which is silent data loss on the same path.
