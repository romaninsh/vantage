# ActiveEntity::save() replaces, ActiveRecord::save() patches — same API name, different semantics

- **Severity:** medium
- **Category:** inconsistencies
- **Location:** `vantage-dataset/src/record.rs:32-34` vs `vantage-dataset/src/record.rs:114-116`

The two change-tracking wrappers expose an identical `save()` method with different persistence semantics: `ActiveEntity::save()` calls `replace` (idempotent full overwrite, creates if missing, removes absent fields), while `ActiveRecord::save()` calls `patch_value` (fails if the row was deleted, merges fields and never removes any). So deleting a key from an `ActiveRecord` and calling `save()` silently leaves the key in storage, and saving after a concurrent delete errors for records but resurrects the row for entities. Neither doc comment mentions the difference; both just say "Save the current state of the record back to the dataset."

```rust
// ActiveEntity
pub async fn save(&self) -> Result<E> {
    self.dataset.replace(&self.id, &self.data).await
}
...
// ActiveRecord
pub async fn save(&self) -> Result<Record<D::Value>> {
    self.dataset.patch_value(&self.id, &self.data).await
}
```

**Recommendation:** Use the same operation for both (replace is the natural "save full state" choice since both wrappers hold the complete value), or rename/document one of them explicitly (e.g. `save_patch()`).
