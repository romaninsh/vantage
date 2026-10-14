# Working on Diorama in Rust

Two readers land here. The first writes Rust that uses a Dio without a UI: an
API handler, a CLI, a batch job. That part is short, because a Dio's facade is
an ordinary Vista. The second works on the crate itself, and needs to know
where things live, which features exist, how the tests are laid out and what
guards the public API. Most of this document is for them.

## Using a Dio from business code

Business code doesn't need sceneries. It wants reads that are fast and writes
that confirm, and `dio.cached()` gives it both: a plain `vantage_vista::Vista`
whose reads are served from the Dio's cache and whose writes go through the
Dio's write queue.

```rust
use vantage_dataset::prelude::{ReadableValueSet, WritableValueSet};

let products = dio.cached();
let all = products.list_values().await?;
let one = products.get_value("sku-1234").await?;
let page = products.fetch_window(0, 50).await?;

products.insert_value("sku-9", &record).await?;
products.delete("sku-9").await?;
```

A facade write returns once it is confirmed, or with its error once it is
rolled back. It goes through the same path as `dio.flash(..)`: staged in the
cache, written through the Lens's writer or the master, settled. A write that
would land a row outside the Dio's set is refused before anything is staged.

A narrowed facade (`with_condition`, `with_search`) reads from the master when
the master took the clause, so the answer covers the whole set and not only
what the cache holds; an order alone never moves a read off the cache. On an
augmented Dio every read is from the cache, because the augmented columns exist
nowhere else; a read that reaches rows not yet hydrated blocks until they are,
after publishing `DioEvent::Hydrating { pending }` for a progress line.

The calls that move data in and out of a Dio:

| Call | What it does |
|---|---|
| `dio.refresh().await?` | Re-read the master: the Lens's refresher, or the loader's way of filling the cache. |
| `dio.flash(ChangeFlash::..).await?` | One write, staged, written through and settled. |
| `dio.apply_change(ChangeEvent::..).await?` | Apply one upstream change (a webhook, a LIVE message) to the cache and announce it. |
| `dio.reconcile(snapshot).await?` | Make the cache hold exactly `snapshot`, leaving rows with a flash in flight alone. |
| `dio.import_values(rows, progress).await?` | Bulk insert: the master's native import when it has one, else one flash per row. |

Skip Diorama for a one-shot tool or a store that is already local and fast. A
Lens pays off in a long-lived process with read amplification, a slow or
flaky source, or several views that should agree on one row.

## The module map

`src/lib.rs` keeps every implementation module private and re-exports the
public types at the root. A type has one path, `vantage_diorama::Dio` and not
a module path, because the module that defines it is private; rustdoc links
in this repository use that root path too. Moving a type between private
modules is therefore never a breaking change.

| Module | Visibility | What it holds |
|---|---|---|
| `lens` | private | `Lens`, `LensBuilder`, `Loader`, `Refresher`, `Tuning`, `Cache`, the cache backends (`RedbCache`, `MemoryCache`), `LoaderCx`, `ChunkSink`, and the Lens's registry of live Dios |
| `dio` | private | `Dio`, `DioBuilder`, `DeriveBuilder`, `DioEvent`; the shared `DioInner` with its registry of shared rows; the facade `DioShell`; `flash` and the write worker; refresh, live follow, import, augmentation passes and the augment scheduler |
| `scenery` | private | the three view traits and their builders; `scenery/table/` holds `TableView` (one owner's view), `SharedRows` (the rows views of one query share), the loader pipeline, the reactor, two-pass loading and `ViewStats` |
| `servo` | private | `Servo`, the editing companion |
| `ops` | private | `ChangeEvent`, `ChangeFlash`, `FlashKind`, `FlashOutcome`, `FlashRejection` |
| `augment` | private | `Augmentation` and its parts (`Detail`, `Source`, `Fetch`, `MergeRule`) |
| `aggregate` | public | aggregations, `Dio::value_scenery` and `Dio::aggregate` |
| `debug` | public with `diagnostics` | the debug tap and process stats |
| `stats` | public with `diagnostics` | live-instance counts, the fetch ledger, the session summary |
| `rhai` | public with `rhai` | `Servo` as a Rhai type |
| `prelude` | public | `Aggregation`, `ChangeEvent`, `ChangeFlash`, `Dio`, `Lens`, the three view traits, `Servo` |

`debug` and `stats` are compiled into every build, because the crate calls the
tap and the counters everywhere; without `diagnostics` they are private
modules and the tap stays off. [ARCHITECTURE.md](ARCHITECTURE.md) walks the
same modules from the inside.

## Features

| Feature | Turns on |
|---|---|
| `diagnostics` | `LensBuilder::debug_datasource`, `Dio::diagnostics`, `Dio::debug_tap`, the public `debug` and `stats` modules |
| `rhai` | `vantage_diorama::rhai` (`register_servo_onto`, `ServoVocab`); pulls `vantage-vista/rhai` |
| `test-util` | hooks for harnesses that watch a Dio's internals, such as `Dio::take_write_worker_handle` |

None is on by default. Build and test with `--all-features`; a test that needs
a feature carries `#[cfg(feature = "..")]`, or its binary lists the feature in
`required-features`.

## Tests

### Layout

- **Unit tests** sit beside the code as `src/**/tests.rs` (or a `mod tests`
  at the bottom of a small file) and reach private items.
- **Integration binaries** are `tests/<name>.rs` for one file, or
  `tests/<name>/main.rs` with topic modules beside it once a binary outgrows a
  file (`tests/record_scenery/{main,builder,reads,writes}.rs`). Cargo finds both
  shapes; only a binary with `required-features` or a custom path needs a
  `[[test]]` entry in `Cargo.toml`.
- **Shared fixtures** live in `tests/support/`. Its root stays
  `support/mod.rs`, because a `tests/support.rs` would be built as a binary of
  its own; its groups are children (`lenses.rs`, `views.rs`, `masters.rs`,
  `aggregate.rs`, and so on). A flat binary declares `mod support;`; a nested
  one declares `#[path = "../support/mod.rs"] mod support;`.
- **BDD scenarios** are `tests/features/*.feature`, driven by `tests/bdd.rs`
  (a `harness = false` binary needing `diagnostics` and `test-util`) with its
  steps and world in `tests/bdd_support/`.
- **Snapshots** for the event-log tests are `insta` files in
  `tests/snapshots/`.

Fixtures belong in `tests/support`; a test binary that grows its own copy of a
master or a Lens is a duplicate waiting to drift.

### Running tests by name

Run the tests that cover a change by name rather than the whole suite. A test
in a nested binary is named by its module path inside that binary; a unit test
by its full path in the crate:

```sh
cargo nextest run -p vantage-diorama --all-features --test owned_views -E 'test(=a_set_on_one_view_never_reshapes_another)'
cargo nextest run -p vantage-diorama --all-features --test record_scenery -E 'test(=builder::load_on_miss_fetches_the_row_and_caches_it)'
cargo nextest run -p vantage-diorama --all-features --lib -E 'test(=scenery::table::cap::tests::a_tail_reads_the_end_of_the_set)'
cargo nextest list -p vantage-diorama --all-features --test two_pass
```

`nextest list` is how to check a split lost nothing: list the binary before
and after moving tests, under `--all-features` and under the default features
when the file has feature-gated tests, and compare the counts.

## The public API snapshot

`api/vantage-diorama.txt` is the crate's public surface as `cargo-public-api`
prints it: every public item, its path, its signature and its auto traits.
`.github/workflows/public-api.yaml` regenerates it and fails on any difference,
so a change to the public API shows in review as a diff of that file.

```sh
mise run api-snapshot
bash api/rules.sh
```

`mise run api-snapshot` runs `api/snapshot.sh`, which pins the nightly and the
`cargo-public-api` version it needs (rustdoc JSON changes between nightlies).
Commit the regenerated file with the change that caused it.

`api/rules.sh` checks the rules the snapshot has to keep:

- A handle or view type (`Dio`, `Lens`, the builders, the sceneries, the
  caches) has no public field, so its state can change without breaking a
  caller.
- No crate with a snapshot allows dead code in `src/lib.rs`.
- A public enum, or a struct with a public field, is `#[non_exhaustive]`
  unless `CLOSED` lists it (`Generation` and `SortDirection` are).
- Every item has one path, inside this crate, and only the prelude
  re-exports.

### Why snapshot the API instead of reviewing it?

Because a re-export lost while splitting a module, or a field made public in
passing, looks like nothing in a source diff. In the snapshot it is a removed
or added line. The rules script then catches the mistakes a reviewer would only
catch by knowing the rule, such as an enum gaining a variant without
`#[non_exhaustive]`.

## House rules

- No `mod.rs` except where a directory has to be its own root
  (`tests/support/mod.rs`, `tests/bdd_support/mod.rs`); a module `x.rs` keeps
  its children in `x/`.
- Files stay around 250 lines. Split by concern, and name the child after what
  it holds (`loader/commit/totals.rs`, not `commit2.rs`).
- `use` imports at the top of the file.
- Doc comments where they carry weight, written for a reader who sees the code
  cold: no notes about what the code used to do.
- `src/lib.rs` denies `clippy::todo` and `clippy::unimplemented` outside
  tests.

## Checklist

- Re-export a new public type at the root, and nowhere else but the prelude.
- Mark a new public enum, or a struct with public fields, `#[non_exhaustive]`.
- Regenerate `api/vantage-diorama.txt` and run `bash api/rules.sh`.
- Put new fixtures in `tests/support/`.
- Run the covering tests by name, with `--all-features`.
- Compare `nextest list` counts before and after moving tests.
