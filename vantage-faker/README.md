# vantage-faker

Synthetic, optionally-live datasource for Vantage. Generates realistic rows and — for
live effects — keeps mutating them, pushing genuine change-events so a subscribed Dio
animates inserts and expiries. For testing and demos, without a real backend.

> Incubating: API may change.

## Effects

- `static` — generate rows once; never change.
- `fifo` — insert one row at a time (newest-first), expire each after a random retention.

## Example

```rust
use std::time::Duration;
use vantage_faker::{FakerColumn, FakerTable, FifoEffect};

let columns = vec![
    FakerColumn { name: "id".into(),    ty: "string".into(),  flags: vec!["id".into()] },
    FakerColumn { name: "email".into(), ty: "string".into(),  flags: vec![] },
    FakerColumn { name: "amount".into(), ty: "decimal".into(), flags: vec![] },
];

let table = FakerTable::build(
    "events",
    columns,
    "id",
    Box::new(FifoEffect {
        interval: Duration::from_secs(1),
        retention_lo: Duration::from_secs(30),
        retention_hi: Duration::from_secs(60),
    }),
);

// `table.vista` lists the current rows; `table.events.subscribe()` streams live deltas.
```

Values are drawn from the [`fake`](https://crates.io/crates/fake) crate: the column name
is matched first (`email`, `name`, `phone`, `city`, …), then the declared type.

## License

MIT OR Apache-2.0
