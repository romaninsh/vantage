# Faker Sims

`vantage-faker` fills memory tables with generated rows. With its `sim` feature it also runs Rhai
scripts as simulations over the same memory store: each sim is a script with its own clock, and a
`SimDef` spawns copies of it (a burst at start, then a rate per sim minute, up to a maximum alive).
Sims write through the data vocabulary from this part of the book, so a Dio, a grid or an API
watching the store sees the rows arrive, change and leave.

<!-- toc -->

---

## A sim script

A faker sim is a Rhai script that runs from top to bottom on its own thread, with its own sim
clock. Its local variables are its state, and `sleep` pauses it. The data vocabulary is there in
full, over the engine's memory store, and `table()` with no name is the sim's own table:

<!-- tested: vantage-faker sim::tests::guide::order_sim_from_the_guide -->
```rhai
if table().where("status", "Placed").count() >= 40 {
    done();
}

let id = table().insert(#{ customer: fake("name"), total: rand_float(5.0, 80.0), status: "Placed" });
sleep(minutes(rand_int(5, 20)));

table().patch(id, #{ status: "Shipped" });
table("order_event").insert(#{ order: id, note: "shipped" });
sleep(hours(2));

table().delete(id);
```

## Sim words

On top of the data vocabulary, sims get:

- **data**: `fake_row()` on a handle (`table().fake_row()`, `table("t").fake_row()`) returns a map
  with a generated value for each column the table declares through `SimEngineBuilder::columns`,
  without the id column. Sequential generators (`walk`, evenly spread `date`) advance one step per
  call, shared by every sim of the def.
- **time**: `seconds(n)`, `minutes(n)`, `hours(n)`, `days(n)`, `sleep(d)`, `wait_until(t)`,
  `now()`, `now_secs()`, `wall_now()`, `wall_in(d)`, `elapsed()`, `clock()`, `done()`.
- **random**: `pick`, `pick_weighted`, `rand_int`, `rand_float`, `chance(p)`, `pattern("BA####")`,
  `sentence(min, max)`, `fake(kind)`, `date_between(from, to)`.
- **spawn**: `spawn_sim(name, args?)`, `sim_id()`, `sim_name()`, and the spawner's `args`.
- **geo**: `great_circle`, `bearing`, `interpolate`.

## The store underneath

A sim's tables are memory Vistas, so reads and writes finish on the first poll and sims run
without tokio (see [the bridge](./hosts.md#async-reads-from-a-synchronous-script)). Writes
broadcast the store's change events to any watching Dio. During a warm start, each written table
stays quiet and sends one `Reset` at the end. A missing table is a script error: the store's
tables are created by the dataset, not by the sim.

The `vantage-faker` README covers defs, spawners, clocks, the operations budget and the built-in
sims.
