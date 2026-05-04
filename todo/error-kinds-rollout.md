# Error kinds — global rollout

[`vantage_core::ErrorKind`](../vantage-core/src/util/error.rs) was added so
higher layers can react to errors based on classification (HTTP 4xx vs
5xx, Sentry alerting policy, UI affordances, etc.). Kinds are set after
construction via modifier methods on `VantageError`:

```rust
error!("...", method = "...", capability = "can_X").is_unsupported();
error!("...", method = "...", capability = "can_X").is_unimplemented();
error!("table has no columns", table = name).is_incorrect_usage();
```

Each modifier emits a `tracing::error!` event with the message + structured
context, so observability sinks pick up the classification automatically.

Currently most call sites still produce `ErrorKind::Generic` (the default).
This document tracks the migration.

## How to apply

For each callsite below, decide which kind fits and append the modifier:

- **`Unsupported`** — operation legitimately not provided. The driver/impl
  honestly doesn't do this. Caller should have checked capability flags
  first; reaching this means they didn't.

- **`Unimplemented`** — operation is intended to exist here but isn't built
  yet. Distinct from `Unsupported` — `Unsupported` says "never will";
  `Unimplemented` says "not yet". The `VistaSource::default_error` helper
  already produces this kind when a capability flag is `true` but the
  trait method isn't overridden.

- **`IncorrectUsage`** — caller violated a contract. Invalid argument
  combinations, missing required state, two parts of the framework
  disagreeing.

If unsure, leave as Generic — better to defer than mislabel. Generic
errors don't auto-trace; the caller decides whether to log them.

## Unsupported

Caller asked for an operation the impl can't perform — by design.

- [ ] `vantage-csv/src/table_source.rs:130–155` — `Sum/Max/Min not implemented for CSV backend`
- [ ] `vantage-csv/src/table_source.rs:167–221` — `CSV is a read-only data source` (×6)
- [ ] `vantage-api-client/src/table_source.rs:178–202` — `Sum/Max/Min not implemented for API backend`
- [ ] `vantage-api-client/src/table_source.rs:215–269` — `REST API is a read-only data source` (×6)
- [ ] `vantage-mongodb/src/mongodb/impls/expr_data_source.rs:14` — expression-form not supported

## Unimplemented

Driver advertised a capability without overriding the matching trait method,
or scaffolding placeholders that will be filled in. `VistaSource::default_error`
already produces this kind for the capability case; nothing to migrate
there. New scaffolding should use `.is_unimplemented()` directly.

## IncorrectUsage

Two parts of code disagree, required state missing, invalid arg combinations.

- [ ] `vantage-mongodb/src/mongodb/impls/selectable_data_source.rs:43` — `MongoSelect has no collection set`
- [ ] `vantage-mongodb/src/mongodb/impls/table_source.rs:171` — `Document missing _id field`
- [ ] `vantage-mongodb/src/mongodb/impls/table_source.rs:354` — `Inserted row disappeared`
- [ ] `vantage-mongodb/src/mongodb/impls/table_source.rs:380` — `Replaced row disappeared`
- [ ] `vantage-mongodb/src/mongodb/impls/table_source.rs:406` — `Record not found after patch`
- [ ] `vantage-mongodb/src/mongodb/impls/table_source.rs:482` — `No collection name for related_in_condition`
- [ ] `vantage-mongodb/src/types/mod.rs:128` — `Expected document in array result`
- [ ] `vantage-mongodb/src/types/mod.rs:134` — `Expected document or array result`
- [ ] `vantage-api-client/src/api.rs:217` — `API data item is not an object`
- [ ] `vantage-api-client/src/api.rs:253–262` — bare-array shape errors

## Future kinds — leave Generic for now

We may add more kinds (`Validation`, `External`/`Io`, `NoData`, `Timeout`,
`AuthRequired`) when patterns become clearer. Until those exist, the
following bucket of callsites stays Generic:

### Validation candidates

- `vantage-config/src/config.rs:140–195` — schema/config errors

### Io / External candidates

- `vantage-csv/src/csv.rs:59–88` — file open/header/row failures
- `vantage-api-client/src/api.rs:196–209` — HTTP / JSON parse failures
- `vantage-mongodb/src/mongodb/impls/table_source.rs` — most `MongoDB *
  failed` wrappings (find, find_one, count_documents, aggregate, etc.)
- `vantage-live/src/cache/redb.rs:68–194` — redb backend errors

### NoData candidates

- Errors that include "Record not found" / "no such row" without the
  inserted-then-disappeared bug shape.

## Tracking

When a callsite is migrated, tick its checkbox here and remove it next
time this list is touched. New callsites should start out classified;
this list shouldn't grow.
