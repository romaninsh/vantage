# vantage-faker

Seeded, relational, live test data for [Vantage](https://github.com/romaninsh/vantage).

vantage-faker fills [`vantage-memory`](https://docs.rs/vantage-memory) tables with artificial
rows that follow rules you give per column. vantage-memory is a real in-process storage
with indexes, queries and change events for Vantage (a datasource). Anything built on Vantage
(vantage-ui, a Vista or a Dio consumer) will be able to interpret and display this data.

Vantage UI implements faker behind `type: faker` in a datasource YAML: the app provides
a `config::DatasetSpec` to vantage-faker.

## How it differs from Faker-style libraries

Libraries such as `fake`, Faker.js or Python Faker produce one value at a time: a name, an email,
a city. User must heavy-lift into generating entire data-set manually. While vantage-faker
uses `fake` as an underlying mechanism, the focus of this framework is on providing:

- **Whole datasets.** Declaratively specify entire table, that reference each other. A reference column always points at a real
  row, and a fan-out gives each parent a bounded number of children. Tables are generated in
  reference order.
- **Reproducible.** One seed gives the same rows on every run, across every table.

## Live and not-so-live data

Vantage Faker will also provide data that can be modified in run-time, relying on
`vantage-memory` backend. You can read, edit and watch it for changes.

The optional `sim` feature can mutate data in real-time. Define rhai scripts to run
in the background as small data mutators, constantly changing the shape of yoour data.
This makes vantage-faker ideal for testing real-time data modification and how UI and
API will adapt to those changes.

Of course sometimes we need adversity and the data may be not-so-live or partially
available. `ShapedShell` can make a table slow, paged or flaky on purpose, so you
  can test a consumer against something harder than an instant in-memory table.

## Basic Example

The smallest use is a table with several columns:

```rust
use vantage_faker::{DatasetGen, FakerColumn, TableGen};
use vantage_memory::MemoryStore;

let store = MemoryStore::new();
let client = TableGen::new("client")
    .column(FakerColumn::new("name", "string"))
    .column(FakerColumn::new("email", "string"))
    .count(50);
DatasetGen::new(Some(42)).table(client).generate(&store)?;

assert_eq!(store.table("client").len(), 50);
```

`generate` builds every row of every table before it writes any, so a bad plan returns an error
and leaves the store untouched. Each table is written quietly: anyone watching it gets a single
`Reset` once its rows are in, not one event per row. The id column is `id` unless you set
another, and seeded ids are zero-padded 20-digit strings (`00000000000000000000`, …), so text
order is row order. The seed (`42`) gives the same rows on every run; `DatasetGen::new(None)`
gives fresh ones.

### Reading the data back

The store is an ordinary vantage-memory datasource, so you read it the way you read any Vantage
table: describe an entity, point a `Table` at the store, and list it.

```rust
use vantage_dataset::prelude::*;
use vantage_memory::MemoryStore;
use vantage_memory::prelude::*;
use vantage_table::table::Table;
use vantage_types::entity;

#[entity(MemoryType)]
#[derive(Debug, Clone, Default)]
struct Client {
    name: String,
    email: String,
}

let clients = Table::<MemoryDB, Client>::new("client", MemoryDB::from_store(store))
    .with_id_column("id")
    .with_column_of::<String>("name")
    .with_column_of::<String>("email");

for (id, client) in clients.list().await? {
    println!("{id}: {} <{}>", client.name, client.email);
}
// 00000000000000000000: Kristian Dare <ryder@example.org>
// 00000000000000000001: Kaylee Price <annabel@example.net>
// …
```

The same `Table` filters, sorts, edits and follows relations. From here on it is plain Vantage:
the [Vantage Book](https://romaninsh.github.io/vantage/) walks through it, starting with
[Tables and Typed Data Access](https://romaninsh.github.io/vantage/intro/step2-tables.html).

## Column rules

A `FakerColumn` is a name, a declared type and an optional generator. Each cell comes from the
first rule that applies:

1. **An explicit `ColumnGen`**, set with `with_generator`.
2. **The column name.** The name is split into lowercase words on `_`, `-`, `.` and camelCase,
   and adjacent words also match joined (`first_name`, `firstName` and `firstname` are the same).
   A word of `email`, `firstname`, `lastname` / `surname`, `username` / `login` / `handle`,
   `name`, `phone` / `mobile` / `tel` / `telephone`, `city`, `country`, `street` / `address`, or
   `company` / `employer` / `organization` / `organisation` gets a realistic value from `fake`.
   Whole words only: `contact_email` is an email, `hostname` and `hotel` are not matched.
3. **The declared type.** Integers (`int`, `integer`, `number`, `i64`, `bigint`) get
   0..10 000, decimals (`decimal`, `float`, `double`, `money`, `amount`, `f64`) two-place
   numbers below 10 000, `bool` a boolean, `date` / `datetime` / `timestamp` a random time in
   the 90 days before the generator's `now` (pin it with `ValueGen::with_now` for repeatable
   output). Anything else gets a lorem word.

The generators, shown in their YAML form:

| Generator | Produces |
|---|---|
| `pick: { values: [Open, Paid], weights: [3, 1] }` | one of the values, optionally weighted |
| `range: { min: 1, max: 500, decimals: 2 }` | a number; an integer without `decimals` |
| `date: { from: -90d, to: now, spread: even }` | an RFC 3339 time; `even` spreads rows in id order |
| `sentence: { min_words: 4, max_words: 10 }` | a lorem sentence |
| `pattern: "BA####"` | a code: `#` digit, `?` letter, `*` either |
| `walk: { start: 100, step: 2.5, min: 0 }` | a random walk over rows: a time series |
| `tree: { roots: 3, depth: 4 }` | a parent id in the same table: a hierarchy |

`walk`, `tree` and even-spread `date` depend only on the seed and the row's position, so adding
rows does not change earlier ones. Two table settings roughen the output: `weirdness(0.05)`
replaces that fraction of string cells with very long, blank, duplicate or emoji labels, and
`extra_fields(ExtraFields { count, size })` adds undeclared filler columns to every row, like an
API that returns more than you asked for.

## Related tables

A reference column holds ids of another table in the same `DatasetGen`. Without a fan-out, rows
spread across parents; with one, every parent gets between `min` and `max` children and the
child count follows from that.

```rust
use vantage_faker::{ColumnGen, DatasetGen, FakerColumn, FanOut, TableGen};

let status = ColumnGen::Pick { values: vec!["Open".into(), "Paid".into()], weights: None };
let invoice = TableGen::new("invoice")
    .column(FakerColumn::new("client_id", "string"))
    .column(FakerColumn::new("status", "string").with_generator(status))
    .reference("client_id", "client")
    .fan_out(FanOut { column: "client_id".into(), min: 1, max: 5 })
    .indexed(["client_id"]);
let client = TableGen::new("client").column(FakerColumn::new("name", "string")).count(10);

DatasetGen::new(Some(42)).table(invoice).table(client).generate(&store)?;
```

Declaration order does not matter: `client` is generated first. A reference to an undeclared
table, a reference cycle or a fan-out on a non-reference column fails with the table named.

## Sims (`sim` feature)

A sim is a Rhai script that runs top to bottom on its own thread. Its local variables are its
state, and `sleep` pauses it on its own sim clock. A `SimDef` gives the script, the table it
writes by default, a spawner (`burst` at start, then `rate_per_min`, at most `max` alive), a
clock speed and an optional warm start:

```rust
use std::time::{Duration, SystemTime};
use vantage_faker::{DatasetGen, SimDef, SimEngine, TableGen};

const ORDER: &str = r#"
    let id = table().insert(#{ customer: fake("name"), total: rand_float(5.0, 80.0), status: "Placed" });
    sleep(minutes(rand_int(5, 20)));
    table().patch(id, #{ status: "Shipped" });
    sleep(hours(2));
    table().delete(id);
"#;

DatasetGen::new(Some(7)).table(TableGen::new("order")).generate(&store)?;

let order = SimDef::new("order", "order", ORDER)
    .with_spawn(0, 2.0, 50) // none at start, 2 per sim minute, 50 alive at most
    .with_clock(60.0) // one real second is a sim minute
    .with_warm(Duration::from_secs(3600));
let engine = SimEngine::builder()
    .store(&store)
    .sim(order)
    .seed(7)
    .manual_clock(SystemTime::now()) // time moves only on engine.advance(..)
    .on_sim_error(|def, err| eprintln!("sim {def}: {err}"))
    .start()?;

assert!(store.table("order").len() > 0); // the warm hour already ran
engine.advance(Duration::from_secs(1));
```

The verbs a script can call:

- **Data:** Vantage's data vocabulary over the engine's store. `table()` is the def's own table
  and `table(name)` any other; both narrow (`where`, `sort`, `search`, `limit`, `ref`), read
  (`get`, `list`, `first`, `count`, `ids`) and write (`insert`, `upsert`, `patch`, `delete`,
  `import_from`), and open `record` drafts. `table().fake_row()` returns a generated value for
  each column declared with `SimEngineBuilder::columns`, with the table's `weirdness` and row
  count from `SimEngineBuilder::fake_rows`. Ids are strings. `patch` and `delete`
  return `false` for a missing row; `insert` with an existing id is an error, so use `upsert` for
  stable ids. The vocabulary is described in
  [Scripting with Rhai](https://romaninsh.github.io/vantage/rhai.html).
- **Random:** `pick`, `pick_weighted`, `rand_int`, `rand_float`, `chance(p)`, `pattern`,
  `sentence`, `fake(kind)` (a value as a column named `kind` would get), `date_between`.
- **Time:** `seconds`, `minutes`, `hours`, `days`, `sleep`, `wait_until`, `now`, `now_secs`,
  `wall_now`, `wall_in(d)` (the wall-clock moment the sim reaches `now + d`, for honest ETAs),
  `elapsed`, `clock`, and `done()` to end the sim early.
- **Spawn and misc:** `spawn_sim(name, args?)`, `sim_id()`, `sim_name()`, the `args` variable,
  `print` / `debug` to the log.
- **Geo:** `great_circle`, `bearing`, `interpolate`, for things that move on a map.

**Warm start.** A def with a warm span runs that much sim time instantly inside `start()`, so
tables open mid-life rather than empty. Tables are quiet meanwhile and send one `Reset` each at
the end; `on_warm_progress` reports progress.

**Errors.** A sim that throws, or runs more than its operations budget between two sleeps
(`DEFAULT_OPS`, 5 million; per def with `with_ops`), ends as errored. Its rows stay and the other
sims keep running. `on_sim_error` reports each one; `SimEngine::stats()` counts live, spawned,
ended and errored sims and writes. `seed` makes random draws repeatable, though thread timing
can still reorder spawns.

**Built-ins.** In config, `script: "builtin:<name>"` names one of these; `sim::builtin::builtin`
returns the source, whose header lists the arguments and columns.

- `fifo`: one sim per row; a row arrives, stays 20–35 s, and leaves. Ids sort newest first.
- `pulse`: one sim keeping a value per key drifting around a baseline, with a feed of recent
  changes and per-minute totals.
- `flight`: one sim per aircraft, from boarding through cruise to landing, with position,
  altitude and ETA updated every 30 sim seconds.
- `folder_tree`: one sim growing a day's folder of access, error and event log files.

## YAML config

`config::DatasetSpec` is a whole dataset (seed, tables, sims) in YAML shape. Unknown keys are
rejected.

```yaml
seed: 9
tables:
  client:
    count: 20
    columns:
      name: { type: string }
      tier: { faker: { pick: { values: [Gold, Silver], weights: [1, 4] } } }
  invoice:
    references: { client_id: client }
    fan_out: { column: client_id, min: 1, max: 5 }
    indexed: [client_id]
    columns:
      client_id: {}
      total: { type: money, faker: { range: { min: 10, max: 900, decimals: 2 } } }
sims:
  arrivals:
    table: client
    script: "builtin:fifo"
    warm: 10m
    spawn: { burst: 0, rate: 30, max: 200 }
```

`DatasetSpec::generate(&store)` seeds the tables. With `sim` enabled, `start_sims(&store)`
starts the sims, and `sim_builder(&store)` returns the builder first so you can add a manual
clock or callbacks. Sim defaults are `burst: 1`, `rate: 0`, `max: 1`, `clock: 1`; a sim without
`table:` writes the first table.

## ShapedShell

A consumer that only ever meets an instant, complete in-memory table will not show you its
spinners, retries or paging bugs. `ShapedShell` wraps any `TableShell` (usually vantage-memory's
`MemoryTableShell`) and applies a `BackendShape`:

```rust,ignore
let shape = BackendShape {
    capabilities: VistaCapabilities { can_count: true, can_fetch_window: true, ..Default::default() },
    page_size: 50,
    latency: LatencyModel { list: Some(Latency::between(ms(80), ms(400))), ..Default::default() },
    faults: FaultSchedule { error_rate: 0.05, total_lie: 3, boundary_skew: true, ..Default::default() },
    seed: Some(42),
};
let vista = Vista::new("client", Box::new(ShapedShell::new(Box::new(memory_shell), shape)));
```

The shell advertises only the listed capabilities and refuses the rest. Latency is set per
operation (list, get, window, count, plus extra for searches). Faults include an error rate,
scheduled outages (`Offline`), cursor expiry, totals that are off by a fixed amount and ±1
offset skew. The seed replays jitter and faults; it does not affect row values.
`tests/shaped_backends.rs` shows complete setups.

## Features

- default: generators, `DatasetGen`, `ShapedShell`.
- `sim`: the Rhai sim engine; adds `vantage-rhai`.
- `serde`: no effect. The `config` module and the `ColumnGen` / `ExtraFields` deserializers are
  always built; the feature stays so existing `features = ["serde"]` requirements resolve.

## License

MIT OR Apache-2.0
