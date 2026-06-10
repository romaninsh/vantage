# README and book claim "0.4" while all crates are 0.5.x; install snippet pins "0.4"

- **Severity:** high
- **Category:** inconsistencies
- **Location:** `README.md:90` (also `README.md:682-697`, `docs4/book.toml:5`, `docs4/src/introduction.md:10`)

README (refreshed 2026-06-07) states "Vantage `0.4` is the current version" and the Installation section tells users to add `vantage-core = "0.4"` etc. Actual workspace versions are 0.5.x (vantage-core 0.5.0, vantage-table 0.5.7, vantage-sql 0.5.9, vantage-surrealdb 0.5.9, vantage-csv 0.5.3), bumped in commits like "Bump versions and changelogs: vista 0.5.3, table 0.5.7, sql 0.5.8". Because these are 0.x crates, semver `"0.4"` resolves to `>=0.4, <0.5`, so the documented install pulls outdated crates whose APIs differ from everything else the README/book describe. The book title ("Vantage 0.4 Framework") and `introduction.md` carry the same stale version.

```
Vantage `0.4` is the current version — a global rewrite starting with the type system.
...
# Core crates
vantage-core = "0.4"
vantage-types = "0.4"
...
vantage-sql = { version = "0.4", features = ["sqlite"] }  # also: "postgres", "mysql"
```

**Recommendation:** Bump every version mention to 0.5 (or use unpinned guidance like `cargo add vantage-sql --features sqlite`, as the book already does), and update the book title/introduction or state explicitly that 0.4 docs also cover 0.5.
