# vantage-faker

Synthetic, optionally-live datasource for Vantage. Generates realistic rows and — for live effects —
keeps mutating them, pushing genuine change-events so a subscribed Dio animates inserts and
expiries. For testing and demos, without a real backend.

> Incubating: API may change.

## Effects

- `static` — generate rows once; never change.
- `fifo` — insert one row at a time (newest-first), expire each after a random retention.
- `PulseSim` — a config-driven live aggregate feed driving three coupled tables.
- `LiveFolderSim` — a synthetic, constantly-mutating multi-layer log tree.

## `LiveFolderSim`

Models a "live" log folder structure that grows in real time, all in memory:

- `{date}/access_logs_{HH}/chunk_{NN}.log` — high-volume access log. The active chunk bumps every
  second by `requests_per_sec × bytes_per_request` bytes; when it crosses `chunk_threshold`, a new
  chunk starts (the old stays).
- `{date}/error_logs/{HH:MM:SS}-errors.log` — rare; one file per error occurrence, gated by
  `error_pct_per_sec`.
- `{date}/events/{event_type}.log` — ten event types each with its own 1–10% per-second probability
  of bumping its file by 2000–4000 bytes.

Each folder and file carries `created`/`modified`; modifying a file touches its parent folder (and
ancestors up to the root) so a parent reflects its newest child. `backfill` replays the algorithm at
full speed from `now − backfill` to `now` on construction before real-time ticks begin.

Two Vistas come out of one shared run loop:

- **Listing** (`listing_vista(path)`): one row per child of a path —
  `{name, kind, size, created, modified}`. Patched in place on each tick via `ChangeEvent`s so a
  subscribed Dio doesn't re-list.
- **Folder size** (`size_vista()`): `{path, size, file_count}` — **get-only**, no list. Fetched with
  100ms–1s latency scaled by file count, the exact slow-get shape viewport debounce tests need.

```sh
cargo run --example live_folder_cli
```

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

Values are drawn from the [`fake`](https://crates.io/crates/fake) crate: the column name is matched
first (`email`, `name`, `phone`, `city`, …), then the declared type.

## License

MIT OR Apache-2.0
