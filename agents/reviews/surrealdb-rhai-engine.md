# Review — SurrealDB Rhai Engine (PR #274)

## Findings

### Fixed in follow-up commit

- [critical] constructors.rs:121 — `fn_count` was dead code (never registered). Deleted.
- [critical] constructors.rs:85 — `make_expr` missing `String` type support. Added.
- [critical] select_methods.rs:98,104 — `select_only`/`select_value` mutated fields directly instead
  of using builder methods. Added `with_only()` to SurrealSelect builder and refactored.
- [important] operators.rs:56 — Clippy warning: needless borrow on `format!()`. Fixed.
- [important] select_methods.rs:111-136 — Graph traversal used string formatting with `.preview()`
  instead of Expression templates. Refactored to use proper Expression nesting.

### Remaining (future work)

- [important] select_methods.rs:86 — No validation for negative `limit`/`start` values
- [minor] mod.rs:69 — `RhaiIdent::alias()` silently replaces the identifier instead of creating an
  alias expression
- [minor] constructors.rs — Inconsistent error message prefixes across modules
- [minor] tests/ — No error test cases (`.err` files)
- [minor] types.rs:21 — `RhaiExpr` Clone could be derived instead of manual impl

## Verdict

Ship as-is with the critical/important fixes applied. Remaining minor items can be addressed in
follow-ups.
