# Broken links: book step8 link, vantage-table → expressions README, rustdoc-only link in crates.io README

- **Severity:** low
- **Category:** bugs
- **Location:** `docs4/src/new-persistence.md:121` (also `vantage-table/README.md:293`, `vantage-expressions/README.md:21`)

Three dead links verified by resolving every relative markdown link: (1) the persistence guide links "Read Step 8" to `./new-persistence/step8-vista.md`, but the file is named `step8-vista-integration.md` (per SUMMARY.md), so the rendered book yields a 404; (2) `vantage-table/README.md:293` links `[vantage-expressions README](vantage-expressions/README.md)` — relative to the vantage-table directory this resolves to `vantage-table/vantage-expressions/README.md`, which doesn't exist (missing `../`); (3) `vantage-expressions/README.md:21` uses a rustdoc intra-doc path `[expression module documentation](crate::expression::core)` which is a literal broken href on GitHub and crates.io where this README is the crate landing page.

```
**[Read Step 8 →](./new-persistence/step8-vista.md)**
...
[vantage-expressions README](vantage-expressions/README.md) for details on `DeferredFn` and
...
[expression module documentation](crate::expression::core).
```

**Recommendation:** Fix the step8 filename, prefix `../` on the vantage-table link, and replace the intra-doc link with a docs.rs URL; consider adding a link-checker (e.g. lychee or mdbook-linkcheck) to CI.
