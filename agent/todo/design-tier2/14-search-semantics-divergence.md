# `search_table_condition` has wildly different semantics per engine

- **Severity:** medium
- **Category:** inconsistencies
- **Location:** `vantage-sql/src/sqlite/impls/table_source.rs:110`, `vantage-surrealdb/src/surrealdb/impls/table_source.rs:122`, `vantage-mongodb/src/mongodb/impls/table_source.rs:115`, `vantage-csv/src/table_source.rs:65`, `vantage-redb/src/redb/impls/table_source.rs:64`

The same trait method behaves incompatibly across the six data engines, so identical UI search input yields different results (or failures) depending on backend:

- SQLite/Postgres/MySQL: `LIKE %v% ESCAPE '$'` over all columns, case-sensitivity backend-dependent (Postgres casts `::text`, others don't).
- SurrealDB: `string::contains(string::lowercase(...))` — case-insensitive, no wildcard escaping needed.
- MongoDB: case-insensitive `$regex` with metacharacter escaping.
- CSV: returns a 0-param `SEARCH '...'` expression that is silently ignored downstream (returns all rows).
- redb: `panic!("full-table search is not supported")` — a panic, not a `Result::Err`.

**Recommendation:** Define the contract (case sensitivity, which columns, empty-table result) in the trait doc and conform each backend; redb and CSV should return `Result::Err`, never panic or silently no-op.

---

## Progress (2026-06-13)

**Done — the "never panic / never silent no-op" half:**
- Contract written into the trait doc (`vantage-table/src/traits/table_source.rs`): search is a
  server-side capability; backends that can't search must yield an `ErrorKind::Unsupported` error
  when the condition resolves, never a silent match-all or panic. In-memory search is the
  Lens/Diorama layer's job.
- **CSV** now returns an `OP_SEARCH` sentinel that `apply_condition` rejects with an `Unsupported`
  error (was the silent match-all from #12, now closed).
- **redb** already returned a deferred error (not a panic — the `panic!` in the description is
  stale); aligned it to `mark_unsupported()` for a consistent `ErrorKind`.

**Deferred — the case-sensitivity divergence:** SQL backends remain case-sensitive `LIKE` while
Surreal/Mongo are case-insensitive. Standardizing on case-insensitive would change the three SQL
backends' behavior; left for a follow-up since it concerns backends that *do* search server-side and
is a separate normative decision from the silent-failure fix above.
