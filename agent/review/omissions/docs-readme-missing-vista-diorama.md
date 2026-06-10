# Root README omits Vista, Diorama and other shipped crates that the book headlines

- **Severity:** medium
- **Category:** omissions
- **Location:** `README.md:64-88`

The book's introduction dedicates three of six tutorial chapters to `Vista` ("the Universal Data Handle"), `Dio & Lens`, and `Scenery — Reactive Views`, and the persistence guide has a "Step 8: Vista Integration" — yet the root README never mentions `vantage-vista`, `vantage-vista-factory` or `vantage-diorama` in its crate list, feature list, or installation section. Other shipped crates are also undocumented at the top level: `vantage-redb`, `vantage-cmd`, `vantage-aws`, `vantage-log-writer`, `vantage-cli-util`. Several of these also lack crate READMEs (`vantage-redb`, `vantage-vista-factory`, `vantage-log-writer`, and `vantage-core` — the only core crate without one), which matters since READMEs become the crates.io landing pages.

```
With all of the fundamental blocks and interfaces in place, Vantage can be extended in several ways.
First - persistence implementation:

- [`vantage-surrealdb`](vantage-surrealdb/README.md) ...
- [`vantage-sql`](vantage-sql/README.md) ...
- `vantage-csv` ...
- [`vantage-api-pool`](vantage-api-pool/README.md) ...
- `vantage-api-client` ...
```

**Recommendation:** Add a "generic/UI layer" bullet group for vantage-vista, vantage-vista-factory and vantage-diorama (they are the actual basis of the AnyTable claims elsewhere in the README), list the remaining backends (redb, cmd, aws), and add minimal READMEs to vantage-core, vantage-redb, vantage-vista-factory and vantage-log-writer.
