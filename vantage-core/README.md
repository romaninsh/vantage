# Vantage Core

Shared error type, `Result` alias and small utilities used by every other Vantage crate. It has no
Vantage dependencies of its own.

## What it ships

- `VantageError`: an error with a message, the call-site location, key=value context and an
  optional source error. Errors chain, so each layer can add its own context.
- `ErrorKind`: `Generic` (default), `Unsupported`, `Unimplemented`, `IncorrectUsage`, `NotFound`,
  `Conflict`, `Forbidden`. Set it with `mark_unsupported()`, `mark_not_found()` and so on, read it with
  `kind()` or `is_not_found()` and friends. Transports use it to pick a status code instead of
  parsing the message.
- `Result<T>`: shorthand for `std::result::Result<T, VantageError>`.
- `Context`: `.context(err)` and `.with_context(|| err)` on any `Result` whose error implements
  `std::error::Error`, wrapping the original error as the source.
- `error!("message", key = value, ...)` builds a `VantageError` with the call-site location and
  context fields. The older `vantage_error!` takes a message or format string only.
- `Priority` (`Background` by default, or `Essential`): a tokio task-local that says whether
  someone is waiting on the request. Set it once with `Priority::Essential.scope(fut)`; transports read it
  with `Priority::current()` to choose a retry policy.
- `IntoVec<T>`: a small conversion helper trait.

## Example

```rust
use vantage_core::{Context, VantageError, error};

fn fetch_client_data(client_id: i32) -> Result<String, VantageError> {
    let table = "client";
    read_database(table).context(error!(
        "Failed to read from database",
        table = table,
        client_id = client_id
    ))
}
```

`cargo run --example error -p vantage-core` prints a full error chain, both inline and with
`report()`.

## Features

- `colored-errors` (default): colored output when an error chain is printed.
- `anyhow-compat`: adds the optional `anyhow` dependency. The crate defines no conversions for it
  yet.

## Integration

All of `vantage-types`, `vantage-dataset`, `vantage-expressions`, `vantage-vista`, `vantage-table`,
`vantage-vista-factory` and the drivers return `vantage_core::Result`, so errors from different
layers and backends chain into one `VantageError`.
