# Moving to PostgreSQL

A file on disk is a fine place for a bar to start. When it outgrows one, with concurrent writers, a
second instance or a backup story, you reach for a database server. In most stacks that is a
migration project. Here it is a refactor that adds one file and changes two attributes, and then a
compile flag decides which backend the binary speaks.

The finished crate is [`learn-9`](https://github.com/romaninsh/vantage/tree/main/learn-9): the
same app as `learn-8`, made backend-parametric.

## Name the backend once

`learn-8` wrote `SqliteDB` in a handful of places. The refactor pulls every one of them into a
single alias, so the concrete backend is named in one file:

```rust,no_run
// src/db.rs: the only file that names a database
use vantage_sql::prelude::*;

#[cfg(not(feature = "pg"))]
pub type Db = vantage_sql::sqlite::SqliteDB;
#[cfg(feature = "pg")]
pub type Db = vantage_sql::postgres::PostgresDB;

pub async fn connect() -> VantageResult<Db> {
    #[cfg(not(feature = "pg"))]
    let url = format!("sqlite:{}?mode=rwc", concat!(env!("CARGO_MANIFEST_DIR"), "/products.db"));
    #[cfg(feature = "pg")]
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| vantage_core::error!("DATABASE_URL must be set for the `pg` build"))?;

    Db::connect(&url).await.context("connect db")
}
```

`PostgresDB::connect`, `db.pool()` and `db.vista_factory().from_table(..)` are the same calls as
their SQLite twins, so `connect()` is the whole of the connection difference: a file path on one
side, a `DATABASE_URL` on the other. `db.rs` also holds the `CREATE TABLE`; it is plain SQL both
backends accept.

## Two attributes on the model

The entity marker names whichever backend is compiled in, and the table builder takes the `Db`
alias instead of `SqliteDB`. That is the entire change to the model:

```rust
# use vantage_sql::prelude::*;
# use vantage_types::prelude::*;
# #[cfg(feature = "pg")]
# use vantage_sql::postgres::AnyPostgresType;
# #[cfg(not(feature = "pg"))]
# type Db = vantage_sql::sqlite::SqliteDB;
# #[cfg(feature = "pg")]
# type Db = vantage_sql::postgres::PostgresDB;
// src/product.rs
#[cfg_attr(not(feature = "pg"), entity(SqliteType))]
#[cfg_attr(feature = "pg", entity(PostgresType))]
#[derive(Debug, Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
    pub stock: i64,
}

impl Product {
    pub fn table(db: Db) -> Table<Db, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
            .with_column_of::<i64>("stock")
    }
}
```

The builder *body*, the columns and the id, doesn't move. `#[entity(..)]` takes a list of the
backends a struct serves, not a choice of one: `#[entity(SqliteType, PostgresType)]` generates
both sets of conversions. `learn-9` picks one with `cfg_attr` only so that the build it isn't
running doesn't need the other driver compiled in.

**Everything else is unchanged: `main.rs`, `sim.rs`, the lens, the Dio and the `DioRouter`.** They
were written against `Db` and the portable data-set methods, so they never named a backend. The
reconcile still calls `cx.master().list_values()`; the till still runs the same
`UPDATE … ORDER BY RANDOM()`; the watch still streams `ADDED`/`MODIFIED`.

```admonish tip title="Why the SQL didn't have to change either"
The till uses `$1` placeholders, `ON CONFLICT (id) DO NOTHING` and `RANDOM()`, which SQLite and
PostgreSQL both accept, and the schema uses `BIGINT`, which is SQLite's INTEGER affinity and
Postgres's 64-bit integer. Where the dialects differ (SQLite's `INTEGER` for booleans against
Postgres's `BOOLEAN`, say), the difference lives in the schema strings in `db.rs`, never in the
model or the app.
```

### Why a compile flag and not a runtime switch?

You could switch at runtime. Everything from the master up is a type-erased `Vista`, so a `match`
on the URL scheme that builds the master from either factory would hand the same Dio code either
backend.

The parts below the master are typed by backend, though. `Table<Db, Product>` and the till's
`db.pool()` are a SQLite type or a Postgres type, so a runtime switch would compile both drivers
and write those parts twice, once per arm. `learn-9` serves one backend per deployment, so it takes
the flag: one alias, one driver in the binary. An app that must speak both at once keeps the
`match`, and pays for both drivers.

## Run it on Postgres

Start a local Postgres (the repo ships the script), point `DATABASE_URL` at it, and build with the
flag:

```console
$ vantage-sql/scripts/postgres/start.sh          # docker: postgres:17-alpine on :5433
$ export DATABASE_URL='postgres://vantage:vantage@localhost:5433/vantage'
$ cargo run -p learn-9 --no-default-features --features pg
serving on http://localhost:3009 — try: curl -N 'localhost:3009/api/products?watch=true'
```

The shelf serves and streams as it did on SQLite, with the same routes, the same NDJSON and the
same till:

```console
$ curl -N 'localhost:3009/api/products?watch=true'
{"type":"ADDED","object":{"index":0,"name":"Espresso","price":280,"stock":12}}
{"type":"ADDED","object":{"index":1,"name":"Cappuccino","price":340,"stock":8}}
...
{"type":"MODIFIED","object":{"index":4,"name":"Cheesecake","price":520,"stock":1}}
```

The it-really-is-the-database proof works over Postgres too. Edit a row with `psql` and watch it
land:

```console
$ psql "$DATABASE_URL" -c "UPDATE product SET price=777 WHERE id='p2'"
{"type":"MODIFIED","object":{"index":1,"name":"Cappuccino","price":777,"stock":6}}
```

## What this proves

The reactive stack, cache, events, sceneries and watch server, ran unchanged over a file-backed
SQLite database and a networked PostgreSQL server, and the code that moved between them was one
alias and two attributes. The repo's `vantage-sql/tests/postgres/6_vista.rs` exercises the same
Vista over a real Postgres.

```admonish success title="The fork converges"
The guide's other path wraps a slow, read-only S3 bucket; this one wraps a fast, writable SQL table
that then became a Postgres server. They need different lenses: a cursor walk into redb against a
one-call list into memory, capability injection against none. Above `lens.dio(master).build()`
they run the *same* Dio, the *same* sceneries and the *same* watch adapter. **The reactive layer
does not depend on the backend.** Choose the backend your problem has; the stack above it is the
one you just learned.
```

## Checklist

- One `Db` alias in `db.rs` names the backend; nothing else does.
- `#[cfg_attr(.., entity(..))]` picks the entity marker for the compiled backend.
- `main.rs`, the lens, the refresher and the router don't change.
- The till's statements and the schema stay within the SQL both dialects accept.
- `--no-default-features --features pg` and a `DATABASE_URL` run the same app on Postgres.

There is one thing left on the table. The app reconciles on a one-second timer: it re-reads the
table whether or not anything changed, and it is always up to a beat behind. A file on disk can't
do better; a *server* can. Next, we commit to Postgres and trade that poll for push: the database
tells us the moment a row changes.
