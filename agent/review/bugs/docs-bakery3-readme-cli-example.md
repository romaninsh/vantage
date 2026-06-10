# bakery_model3 README documents a `cli` example that doesn't exist and outdated sources/version

- **Severity:** medium
- **Category:** bugs
- **Location:** `bakery_model3/README.md:43` (also lines 3, 28)

Every CLI invocation in the README uses `cargo run -p bakery_model3 --example cli -- ...`, but the examples directory contains only `0-intro.rs`, `cli-vista.rs`, `contained-surreal.rs` and `dynamo-smoke.rs` — `cargo` errors with "no example target named `cli`". The README also says sources are "`csv`, `surreal`" while `cli-vista.rs` actually supports `csv, sqlite, postgres, mongo, surreal` (and the sibling `CLI_WALKTHROUGH.md` correctly uses `--example cli-vista -- sqlite ...`). Line 3 still labels the crate as "demonstrating Vantage 0.3" although the workspace is at 0.5.x.

```
# List products from CSV files
cargo run -p bakery_model3 --example cli -- csv product list

# List clients from SurrealDB
cargo run -p bakery_model3 --example cli -- surreal client list
```

**Recommendation:** Replace `--example cli` with `--example cli-vista` throughout, update the sources table to the five actually supported backends (matching CLI_WALKTHROUGH.md), and fix the "Vantage 0.3" label.
