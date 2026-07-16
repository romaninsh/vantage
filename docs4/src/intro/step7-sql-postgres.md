# Moving to PostgreSQL

A file on disk is a fine place for a bar to start. When it outgrows one — concurrent writers, a
second instance, a backup story — you reach for a real database server. In most stacks that is a
migration project. Here it is a refactor that touches one new file and two attributes, and then a
compile flag decides which backend the binary speaks.

The finished crate is [`learn-9`](https://github.com/romaninsh/vantage/tree/main/learn-9) — the
same app as `learn-8`, made backend-parametric.

## Name the backend once

`learn-8` wrote `SqliteDB` in a handful of places. The refactor pulls every one of them into a
single alias, so the concrete backend is named in exactly one file:

```rust
// src/db.rs — the only file that names a database
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

`PostgresDB::connect`, `db.pool()`, and `db.vista_factory().from_table(...)` are the same calls as
their SQLite twins — the two backends implement one interface — so `connect()` is the whole of the
connection difference: a file path on one side, a `DATABASE_URL` on the other.

## Two attributes on the model

The entity marker names whichever backend is compiled in; the table builder now takes the `Db`
alias instead of `SqliteDB`. That is the entire change to the model:

```rust
// src/product.rs
#[cfg_attr(not(feature = "pg"), entity(SqliteType))]
#[cfg_attr(feature = "pg",       entity(PostgresType))]
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

The builder *body* — the columns, the id — does not move. `#[entity(...)]` is a list of the
backends a struct serves, not a choice of one; listing `PostgresType` alongside `SqliteType` just
generates a second set of conversions. The same `Product` could serve six backends at once, as
`bakery_model3` does.

**Everything else — `main.rs`, `sim.rs`, the lens, the Dio, the `DioRouter` — is unchanged.** It
was written against `Db` and the portable data-set methods, so it never named a backend to begin
with. The reconcile still calls `master().list_values()`; the till still runs the same
`UPDATE … ORDER BY RANDOM()`; the watch still streams `ADDED`/`MODIFIED`.

```admonish tip title="Why the SQL didn't have to change either"
The till uses `$1` placeholders, `ON CONFLICT (id) DO NOTHING`, and `RANDOM()` — all of which
SQLite and PostgreSQL both accept — and the schema uses `BIGINT`, which is SQLite's INTEGER
affinity and Postgres's 64-bit integer. Where the dialects genuinely differ (SQLite's `INTEGER` for
booleans vs Postgres's real `BOOLEAN`, say), the difference lives in the schema strings in
`db.rs` — never in the model or the app.
```

## Run it on Postgres

Start a local Postgres (the repo ships the script), point `DATABASE_URL` at it, and build with the
flag:

```text
$ vantage-sql/scripts/postgres/start.sh          # docker: postgres:17-alpine on :5433
$ export DATABASE_URL='postgres://vantage:vantage@localhost:5433/vantage'
$ cargo run -p learn-9 --no-default-features --features pg
serving on http://localhost:3009 — try: curl -N 'localhost:3009/api/products?watch=true'
```

The shelf serves and streams exactly as it did on SQLite — same routes, same NDJSON, same till:

```text
$ curl -N 'localhost:3009/api/products?watch=true'
{"type":"ADDED","object":{"index":0,"name":"Espresso","price":280,"stock":12}}
{"type":"ADDED","object":{"index":1,"name":"Cappuccino","price":340,"stock":8}}
...
{"type":"MODIFIED","object":{"index":4,"name":"Cheesecake","price":520,"stock":1}}
```

And the it-really-is-the-database proof works over Postgres too — edit a row with `psql`, watch it
land:

```text
$ psql "$DATABASE_URL" -c "UPDATE product SET price=777 WHERE id='p2'"
{"type":"MODIFIED","object":{"index":1,"name":"Cappuccino","price":777,"stock":6}}
```

## What this proves

The reactive stack — cache, events, sceneries, watch server — ran unchanged over a file-backed
SQLite database and a networked PostgreSQL server, and the code that moved between them was one
alias and two attributes. This is not a toy path: the repo's `tests/postgres/6_vista.rs` exercises
a Vista over a real Postgres, and the `launch-control` example deploys the identical model to AWS
Lambda against Aurora Postgres while defaulting to SQLite locally.

```admonish success title="The fork converges"
The guide's other path wraps a slow, read-only S3 bucket; this one wraps a fast, writable SQL table
that then became a Postgres server. They need different lenses — paged versus eager,
capability-injecting versus not — but above `make_dio` they run the *same* Dio, the *same*
sceneries, and the *same* watch adapter. That is the claim the whole guide was built to earn:
**the reactive layer does not depend on the backend.** Choose the backend your problem has; the
stack above it is the one you just learned.
```

There is still one thing left on the table. The app reconciles on a one-second timer — it re-reads
the table whether or not anything changed, and it is always up to a beat behind. A file on disk
can't do better; a *server* can. Next, we commit to Postgres and trade that poll for real-time
push: the database tells us the moment a row changes.
