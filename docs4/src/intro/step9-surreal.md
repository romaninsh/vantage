# Switching to SurrealDB

Postgres was a migration *within* SQL — a different server, the same dialect family. SurrealDB is a
different database entirely: multi-model, schemaless by default, and reached over a WebSocket rather
than a pooled SQL connection. Adopting it is the real test of the claim the whole path has been
making. The goal here is to change **as little as possible** — and then to cash in the one thing
SurrealDB does better than a SQL server: it can push the *actual change*, not just a ping.

The finished crate is [`learn-11`](https://github.com/romaninsh/vantage/tree/main/learn-11). Set it
next to `learn-10` and the diff is small and boring — which is the result we want.

## What changes

Three touch-points name the backend, and nothing else does.

**The entity marker** — swap the backend in the attribute list:

```rust
#[entity(SurrealType)]           // was: #[entity(PostgresType)]
#[derive(Debug, Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
    pub stock: i64,
    pub created: i64,
}
```

**The table builder** — the same columns, built against `SurrealDB`; SurrealDB's native id is a
`Thing` (`product:⟨key⟩`):

```rust
pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Product> {
    Table::new("product", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("price")
        .with_column_of::<i64>("stock")
        .with_column_of::<i64>("created")
}
```

**Connect and setup** — a DSN instead of a `DATABASE_URL`, and — notably — **no trigger**. Where
Postgres needed a `pg_notify` function and an `AFTER … FOR EACH STATEMENT` trigger, SurrealDB emits
change frames on its own. Setup is just making sure the table exists:

```rust
let client = SurrealConnection::dsn(&dsn)?.connect().await?;
// The one bit of schema. SurrealDB streams changes natively via LIVE SELECT,
// so there is nothing else to wire.
client.query("DEFINE TABLE IF NOT EXISTS product SCHEMALESS", None).await?;
let db = SurrealDB::new(client);
```

The till (the mutator) is the same active-entity code as before; only the id it hands `new_entity`
is a `Thing`:

```rust
table.new_entity(
    Thing::new("product", key),          // was: a String id
    Product { name, price, stock, created },
).save().await?;
```

## What doesn't change

The server's reactive core is untouched. The Vista comes from `SurrealVistaFactory` instead of
`db.vista_factory()`, and then every line is identical to the Postgres chapter — including the one
that matters:

```rust
let mut master = SurrealVistaFactory::new(db.clone())
    .from_table(Product::surreal_table(db.clone()))?;
master.add_order("created", SortDirection::Ascending)?;

let dio = lens.make_dio(master).await?;   // same Lens, same cache, same on_start/on_refresh
dio.watch().await?;                        // the same call — now backed by LIVE SELECT
```

And the **frontend is byte-for-byte the same file** as `learn-10`. The React page consumes
`?watch=true` and animates the shelf; it never learns which database is behind the stream. Diff the
two `frontend/index.html` files and you get nothing — the clearest statement the guide can make that
the reactive layer, up to and including the UI, does not depend on the backend.

## The payoff: precise push, no re-query

`dio.watch()` looks the same, but underneath it is doing something better than it could on Postgres.
SurrealDB's `LIVE SELECT` delivers a **typed, per-row** notification — the action (`CREATE` /
`UPDATE` / `DELETE`) *and* the affected record — for every change. So the Dio applies each change
directly to its cache: a create inserts one row, an update repaints one row, a delete removes one
row. There is no "something changed, re-read everything" step, because the change already arrived in
full.

```text
$ curl -N 'localhost:3011/api/products?watch=true'
{"type":"ADDED","object":{"index":0,"id":{"@@TAGGED@@":[8,["product","d51..."]]},"name":"Mojito","price":1000,"stock":11}}
{"type":"MODIFIED","object":{"index":0,"id":{"@@TAGGED@@":[8,["product","d51..."]]},"name":"Mojito","price":1000,"stock":10}}
{"type":"DELETED","object":{"index":0,"id":{"@@TAGGED@@":[8,["product","d59..."]]},"name":"Boulevardier","price":1400,"stock":1}}
```

(The id arrives as a SurrealDB `Thing` rather than a bare string — the frontend's `idOf` helper
normalises either shape to a stable key, which is why the same UI serves both backends.)

```admonish success title="One capability, three freshness models"
The `product` shelf has now run behind three backends, and the app above `make_dio` never changed:

| Backend | How `dio.watch()` learns of a change | Granularity |
|---|---|---|
| SQLite | a `refresh_every` timer (it can't push) | poll, whole-set |
| PostgreSQL | `LISTEN/NOTIFY` — a payload-less ping → reconcile | **coarse** push, re-read on each write |
| SurrealDB | `LIVE SELECT` — the action + the row | **fine-grained** push, apply the row directly |

Poll, coarse push, fine push — the same `dio.watch()` call, chosen transparently from what the Vista
advertises. You pick the database your problem has; the freshness you get is the best that database
can offer, and the code that consumes it is the code you already wrote.
```

From here the reference half of the book takes over — [SurrealDB](../surrealdb.md) for the driver
details, [SQL](../sql.md) for the dialects, [Config-Driven Vistas](../config-driven-vistas.md) for
declaring all of this from YAML, and [Adding a New Persistence](../new-persistence.md) when your
backend isn't one Vantage already speaks.
