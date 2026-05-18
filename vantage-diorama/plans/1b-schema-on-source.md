# Stage 1b — Schema moves from `Vista` to `TableShell`

Status: **Done**

Cross-cutting refactor that lands between stage 1 (skeleton) and stage 2
(CSV walkthrough). Pulled out of stage 2's discussion phase because it
touches `vantage-vista`'s public surface and every existing `TableShell`
impl, not just diorama.

## Motivation

`Dio::vista()` must return a fresh `Vista` on every call (so callers can
narrow conditions independently). The facade Vista represents
"master, but cached and reactive" — it should expose master's schema
without copying.

Before this refactor, `Vista` owned `columns`, `references`, and
`id_column` as plain fields. The facade either had to copy them or
share them through `Arc`. Either path muddies the model: the schema
isn't really the facade's — it's master's.

After this refactor, the source of truth for schema is the
`TableShell` impl itself. `Vista` becomes a thin holder for name +
foreign resolvers + capabilities + source. The facade `DioShell`
implements the new `TableShell` schema methods by forwarding to
`master.source()`.

## Trait surface change

Three new methods on `TableShell`, **no defaults**:

```rust
fn columns(&self) -> &IndexMap<String, Column>;
fn references(&self) -> &IndexMap<String, Reference>;
fn id_column(&self) -> Option<&str>;
```

Forcing every impl to answer prevents the silent-empty-schema failure
mode.

## `Vista` shape change

Removed fields: `columns`, `references`, `id_column`.

Constructor: `Vista::new(name, source)` — the old `metadata` argument
moves into the shell.

Vista's metadata accessors become one-liners that forward:

```rust
pub fn get_column_names(&self) -> Vec<&str> {
    self.source.columns().keys().map(String::as_str).collect()
}
```

`Vista::source` becomes `pub` so `DioShell` can forward to it.

## Per-driver pattern

Each `XxxTableShell` now stores its own `VistaMetadata`. The matching
factory builds the metadata as before, then passes it into the shell
constructor:

```rust
pub fn from_table<E>(&self, table: Table<Csv, E>) -> Result<Vista> {
    let metadata = metadata_from_table(&table);
    let shell = CsvTableShell::new(table, capabilities, metadata);
    Ok(Vista::new(name, Box::new(shell)))
}
```

The trait-method implementations are trivial getters returning slices
of `self.metadata`.

## Scope (this branch)

In:
- `vantage-vista` — trait change, struct refactor, MockShell + tests
- `vantage-csv` — `CsvTableShell` + `CsvVistaFactory`
- `vantage-diorama` — `DioShell` forwards to master

Out (will be ported later, branched off 0.4):
- `vantage-sql`, `vantage-mongodb`, `vantage-surrealdb`,
  `vantage-log-writer` — vista feature on these breaks, but feature is
  opt-in and not enabled by default
- `vantage-api-client`, `vantage-api-pool` — hard dep on vista, must
  be excluded from workspace temporarily
- `vantage-cli-util` — uses `Vista::new` directly, must be excluded
  temporarily

## Why no version bump

Vista is pre-release / behind the `vista` feature flag on every driver
that exposes it. No external consumer has shipped against the current
shape, so a breaking change to `TableShell` and `Vista::new` is
free at this point. Patch bumps only where workspace path-deps insist.

## Acceptance

- `cargo check --workspace` clean (with the excluded crates removed)
- MockShell tests pass
- CsvVistaFactory builds vistas through the new path
- `vantage-diorama` builds — `DioShell` forwards schema to master
