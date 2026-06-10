# README says PostgreSQL/MySQL are "coming next" but vantage-sql already implements them

- **Severity:** medium
- **Category:** inconsistencies
- **Location:** `README.md:665` (also `README.md:70-71,713`)

The "What's Coming Next" section lists "PostgreSQL / MySQL — Extend `vantage-sql` beyond SQLite", and the crate bullet describes `vantage-sql` as "SQLite implementation (via sqlx)". In reality `vantage-sql/src/` already ships `postgres/` (with `PostgresDB`, `PostgresSelect`) and `mysql/` modules behind cargo features, `bakery_model3/examples/cli-vista.rs` accepts `postgres` as a source, the book's history table credits 0.4 with "SurrealDB, SQLite, Postgres, MySQL, MongoDB, CSV, REST API", and the README's own install snippet says `# also: "postgres", "mysql"`. The README's status table (line 713) likewise scores the SQL column as "SQLite" only. The README contradicts both the code and itself.

```
## What's Coming Next
...
- **PostgreSQL / MySQL** - Extend `vantage-sql` beyond SQLite using sqlx's multi-database support.
```
```
- [`vantage-sql`](vantage-sql/README.md) - SQLite implementation (via sqlx) with full CRUD,
```

**Recommendation:** Move PostgreSQL/MySQL out of the roadmap, describe vantage-sql as SQLite/PostgreSQL/MySQL via sqlx feature flags, and relabel the status-table column accordingly (noting any backend-specific gaps if Postgres/MySQL coverage is partial).
