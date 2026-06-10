# "vantage-ui-adapters" is actually package `dataset-ui-adapters`, unpublished, with unusable Quick Start

- **Severity:** medium
- **Category:** inconsistencies
- **Location:** `vantage-ui-adapters/README.md:22` (also `vantage-ui-adapters/Cargo.toml:2-5`, `README.md:81-82`)

The root README and the crate's own README/Quick Start refer to the crate as `vantage-ui-adapters` and the example imports `use vantage_ui_adapters::...`, but the package is actually named `dataset-ui-adapters` (so the import path is `dataset_ui_adapters`) and it is `publish = false` — it is not on crates.io at all. The Quick Start additionally tells users to depend on it via `path = "."` and on `bakery_model3 = { path = "../bakery_model3" }`, which only works from inside this repository's directory layout. A reader who follows the root README bullet ("Implement DataGrid for Tauri, EGui, Cursive, GPUI, RatatUI and Slint") cannot install or import the crate as documented.

```
[dependencies]
vantage-ui-adapters = { path = ".", features = ["egui"] }
bakery_model3 = { path = "../bakery_model3" }
```
```
[package]
name = "dataset-ui-adapters"
...
publish = false
```

**Recommendation:** Rename the package to `vantage-ui-adapters` (or fix all docs to `dataset_ui_adapters`), state clearly that it is an in-repo reference implementation rather than a published crate, and make the Quick Start use git/clone instructions that actually work.
