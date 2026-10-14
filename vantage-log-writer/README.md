# vantage-log-writer

Append-only persistence for [Vantage](https://github.com/romaninsh/vantage).
Each insert becomes one JSON line in `{base_dir}/{table}.jsonl`. Use it for
audit trails, event logs, or as the write side of a composed data source.

```toml
[dependencies]
vantage-log-writer = { version = "0.6.5", features = ["vista"] }
```

```rust,ignore
use vantage_log_writer::LogWriter;
use vantage_table::table::Table;

let writer = LogWriter::new("./logs");
let events = Table::<LogWriter, Event>::new("events", writer.clone())
    .with_column_of::<String>("name")
    .with_column_of::<i64>("user_id")
    .with_column_of::<bool>("is_admin");

let id = events.insert_return_id(&event).await?;
// ./logs/events.jsonl gets {"name":..,"user_id":..,"is_admin":..,"id":"<ULID>"}
```

## How it writes

- `LogWriter::new(base_dir)` starts a background tokio task. An insert queues
  the record on a channel and returns; the task appends the line. There is no
  fsync and no crash safety.
- Cloning a `LogWriter` (or a table built on it) shares the same channel and
  task.
- Only declared columns are written. Fields not in the column list are dropped.
- Without an id, each row gets a ULID. `with_id_column(col)` changes the id
  column (default `id`).
- Reads (`list_values`, `get_value`, counts) and expression execution return
  `ErrorKind::Unsupported`.

Values use the crate's JSON type system (`JsonType`, `AnyJsonType`,
`JsonTypeVariants`).

## Vista (`vista` feature)

`writer.vista_factory()` returns a `LogWriterVistaFactory`. It builds an
insert-only `Vista` (`can_insert` is the only capability) from a typed table
or from YAML:

```rust,ignore
let vista = writer.vista_factory().from_table(events)?;

let vista = writer.vista_factory().from_yaml(r#"
name: prod_events
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
log_writer:
  filename: events_v2
"#)?;
```

The optional `log_writer.filename` block writes to `events_v2.jsonl` instead
of the table name. Unknown keys in that block are an error.

## Related crates

`vantage-table` and `vantage-vista` provide the traits this crate implements.
The book's "Adding a New Persistence" guide uses it as the insert-only example
in its Vista integration step.
