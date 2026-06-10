# LogWriter table name joins into the path unchecked (traversal out of base_dir)

- **Severity:** low
- **Category:** security
- **Location:** `vantage-log-writer/src/log_writer.rs:56`

`file_path` builds the target file by joining `base_dir` with the raw table name. A table named `../../../tmp/evil` (table names come from YAML/Rhai config, which in an AI-native config-as-code product may be generated or third-party) escapes `base_dir` and appends records to an arbitrary path ending in `.jsonl`; the writer task even runs `create_dir_all` on the parent (`writer_task.rs:55`), creating directories anywhere the process can write.

```rust
pub(crate) fn file_path(&self, table_name: &str) -> PathBuf {
    self.inner.base_dir.join(format!("{}.jsonl", table_name))
}
```

**Recommendation:** Reject table names containing path separators or `..` (or canonicalise the joined path and verify it stays under `base_dir`) before queueing a write.
