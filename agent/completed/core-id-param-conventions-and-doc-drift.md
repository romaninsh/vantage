# Mixed id-parameter conventions across dataset traits, and trait docs showing outdated signatures

- **Severity:** low
- **Category:** inconsistencies
- **Location:** `vantage-dataset/src/traits/dataset.rs:125,189` and `vantage-dataset/src/traits/valueset.rs:46-60`

Within the same trait family, `ReadableDataSet::get` takes `id: impl Into<Self::Id> + Send` while `WritableDataSet::insert/replace/patch` and all `ValueSet` methods take `id: &Self::Id`. Callers must write `table.get("1".to_string())` but `table.insert(&"1".to_string(), ...)` — two conventions for the same concept. Additionally, the `ValueSet` doc example shows `get_value` returning `Result<Self::Value>` while the real signature is `Result<Option<Record<Self::Value>>>` (and `get_some_value` returning `Result<Option<(Self::Id, Self::Value)>>` instead of the `Record`-wrapped form), so the primary implementation example no longer compiles conceptually against the trait.

```rust
// dataset.rs — Into-style
async fn get(&self, id: impl Into<Self::Id> + Send) -> Result<Option<E>>;
async fn insert(&self, id: &Self::Id, entity: &E) -> Result<E>;

// valueset.rs doc example — stale signature
/// async fn get_value(&self, id: &Self::Id) -> Result<Self::Value> {
```

**Recommendation:** Standardize on one id-passing convention (either `&Self::Id` everywhere or `impl Into<Self::Id>` everywhere) across Readable/Writable/Insertable traits, and update the `ValueSet` doc example to the current `Option<Record<...>>` signatures.
