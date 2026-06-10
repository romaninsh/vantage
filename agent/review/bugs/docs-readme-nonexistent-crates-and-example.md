# README references crates and an example file that don't exist (vantage-config, bakery_api, bakery_model/examples)

- **Severity:** medium
- **Category:** bugs
- **Location:** `README.md:84` (also `README.md:531-532,613`)

Three referents in the README don't exist: (1) line 84 lists a `vantage-config` crate ("Reads Entity definitions from yaml, creating type-erased AnyTables") — no such crate exists in this repo or on the workspace member list (the YAML loader is `vantage_inventory` in the vantage-ui repo); (2) lines 531-532 link to `https://github.com/romaninsh/vantage/blob/main/bakery_model/examples/3-query-builder.rs`, but `bakery_model/` contains only `src/` — no `examples/` directory, so the link 404s; (3) line 613 says "there is integration with Axum (see `bakery_api` crate)" — no `bakery_api` directory or crate exists anywhere in the repo (the Axum demo is `learn-3/src/vantage_axum.rs`).

```
- vantage-config - Reads Entity definitions from `yaml` file, creating type-erased `AnyTable`s.
...
(Full example:
<https://github.com/romaninsh/vantage/blob/main/bakery_model/examples/3-query-builder.rs>)
...
there is integration with Axum (see `bakery_api` crate)
```

**Recommendation:** Drop or rename the `vantage-config` bullet, point the query-builder link at an existing example (e.g. `bakery_model3/examples/` or `learn-1`), and point the Axum mention at `learn-3` / the book's "A Standalone Axum Server" chapter.
