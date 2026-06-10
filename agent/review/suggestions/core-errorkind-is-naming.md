# VantageError `is_*` methods are builders, not predicates

- **Severity:** low
- **Category:** suggestions
- **Location:** `vantage-core/src/util/error.rs:236-256`

`is_unsupported()`, `is_unimplemented()` and `is_incorrect_usage()` consume `self`, mutate the error kind, emit a `tracing::error!` event as a side effect, and return `Self`. In Rust, `is_*` universally signals a `&self -> bool` predicate; here `if err.is_unsupported()` doesn't even compile but reads as a check, and the hidden tracing side effect inside an innocuous-looking "getter" name is surprising. Code review of callsites (`error!(...).is_unsupported()`) is harder than it should be.

```rust
/// Mark as [`ErrorKind::Unsupported`] and emit a `tracing::error!`
/// event with the error's message and context.
pub fn is_unsupported(mut self) -> Self {
    self.kind = ErrorKind::Unsupported;
    self.emit_trace();
    self
}
```

**Recommendation:** Rename to builder-style names (`as_unsupported()` / `mark_unsupported()` / `with_kind(ErrorKind::Unsupported)`), keep `is_*` available as real `&self -> bool` predicates, and consider making the trace emission explicit (`.traced()`) so classification and logging aren't coupled.
