# A Standalone Axum Server

Chapter 2 ended with a CLI that prints products to stdout. In this chapter we put the same model
behind an HTTP API:

```sh
curl "http://localhost:3001/categories"
```

```json
[{ "name": "Sweet Treats" }, { "name": "Pastries" }, { "name": "Breads" }]
```

```sh
# Products in a specific category
curl "http://localhost:3001/categories/1/products"

# Writes
curl -X POST "http://localhost:3001/categories" \
  -H 'content-type: application/json' -d '{"name":"Gluten-Free"}'
```

A few things about the shape of this API:

- `/categories/{id}/products` is a **nested** route — products narrowed by the relationship we
  defined in chapter 2. The handler for it is the same generic `list` as `/categories`, just applied
  to a scoped table.
- `/products` and `/categories` start out on SQLite, same as chapter 2. Toward the end of the
  chapter we **migrate** them to MongoDB. That migration is a change to the model file; the handlers
  and routes don't know the difference.

The handler functions are each written **once** and mounted against any `Table<Backend, Entity>` the
router has on hand — one `list`, one `get`, one `post`, one `patch`, one `delete`, parameterised
over the backend and entity types. Adding another entity is a route registration, not a new handler.

---

## The minimum Axum skeleton

Two pieces: a small adaptation to chapter 2's entities so they round-trip through HTTP, then the
server itself. One new dependency:

```sh
cargo add axum
```

Tokio and serde are already pulled in from earlier chapters. `axum` is the HTTP framework; it uses
the existing `serde` to encode response bodies as JSON.

**Entities.** Chapter 2's `Category` carried a computed `title` field; Chapter 2's `Product` carried
a computed `category` field. Both were assembled from subqueries — great for display, but they don't
round-trip cleanly through a `POST` body because the JSON would try to write columns that don't
exist. For a writable API we replace the computed fields with the plain FK column that sits under
them.

`src/category.rs`:

```rust
use serde::{Deserialize, Serialize};

#[entity(SqliteType)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
}

impl Category {
    pub fn table(db: SqliteDB) -> Table<SqliteDB, Category> {
        Table::new("category", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_many("products", "category_id", Product::table)
    }
}
```

`src/product.rs`:

```rust
use serde::{Deserialize, Serialize};

#[entity(SqliteType)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Product {
    pub name: String,
    pub price: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub is_deleted: bool,
}

impl Product {
    pub fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
        let is_deleted = Column::<bool>::new("is_deleted");
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
            .with_column_of::<String>("category_id")
            .with_column_of::<bool>("is_deleted")
            .with_condition(is_deleted.eq(false))
            .with_one("category", "category_id", Category::table)
    }
}
```

Two things to notice:

- `category_id` is now a real column in both the struct and the table, not a computed expression.
  Same information as before, but clients can read it from `GET` and supply it on `POST`.
- `is_deleted` picks up `#[serde(default)]` so `POST /products` bodies don't have to carry it; new
  products default to live.

The soft-delete condition from chapter 2 stays — `GET /products` will still hide rows with
`is_deleted = true`.

Now the server. Replace `src/main.rs` with:

```rust
mod category;
mod product;

use std::sync::OnceLock;

use axum::{routing::get, Json, Router};
use category::Category;
use vantage_sql::prelude::*;

static DB: OnceLock<SqliteDB> = OnceLock::new();

fn db() -> SqliteDB {
    DB.get().expect("database not initialised").clone()
}

async fn list_categories() -> Json<Vec<Category>> {
    let rows = Category::table(db()).list().await.unwrap();
    Json(rows.into_values().collect())
}

#[tokio::main]
async fn main() -> VantageResult<()> {
    let conn = SqliteDB::connect("sqlite:products.db")
        .await
        .context("Failed to connect to products.db")?;
    DB.set(conn).ok();

    let app = Router::new().route("/categories", get(list_categories));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
```

A few notes on what this is doing:

- **`static DB: OnceLock<SqliteDB>`.** The handler needs a database handle but we don't want to open
  a new connection per request. A program-wide `OnceLock` is the simplest possible holder — set once
  in `main`, read from anywhere.
- **Each request calls `Category::table(db())`** — building the table (columns, relationships) and
  then lists it.
- **The handler returns `Json<Vec<Category>>` directly.** No DTO layer — the entity's fields become
  the JSON fields.

Run it:

```sh
cargo run
```

In another terminal:

```sh
curl http://localhost:3001/categories
```

```json
[
  { "name": "Sweet Treats" },
  { "name": "Pastries" },
  { "name": "Breads" }
]
```

---

## Caching table definitions

We can cache each table definition so it's built once and handed out by reference on every
subsequent call. In `src/category.rs`, add `use std::sync::OnceLock;` at the top and wrap the body
of `Category::table`:

```rust
impl Category {
    pub fn table(db: SqliteDB) -> &'static Table<SqliteDB, Category> {
        static CACHE: OnceLock<Table<SqliteDB, Category>> = OnceLock::new();
        CACHE.get_or_init(|| {
            Table::new("category", db)
                // ...columns unchanged...
                .with_many("products", "category_id", |db| Product::table(db).clone())
        })
    }
}
```

Three changes from chapter 2's version:

- **Return type is `&'static Table<...>` instead of `Table<...>`.** The cache owns the definition;
  callers get a shared reference. Most table operations (`list`, `get`, `get_count`, `insert`,
  `replace`, `delete`, `get_ref_as`) take `&self`, so they work on the reference without any clone.
- **`static CACHE: OnceLock<...>` lives inside the function.** A function-local static is still a
  program-wide single instance — Rust allows this and it keeps the cache private to the table that
  owns it.
- **The `with_many` callback is a closure: `|db| Product::table(db).clone()`.** The framework still
  hands us a db and expects an owned `Table<SqliteDB, Product>` back, so we call the cached accessor
  and clone the reference. On the first call this triggers Product's cache to build; every
  subsequent call just clones a pre-built definition.

`src/product.rs` follows the same pattern — wrap the body in `OnceLock::get_or_init`, change the
return type to `&'static Table<SqliteDB, Product>`, and rewrite `with_one` as
`|db| Category::table(db).clone()`.

The handler in `main.rs` doesn't change at all — `Category::table(db())` used to return an owned
`Table`, and now it returns a `&'static Table` that auto-derefs through the same `.list()` call.
Restart the server and hit `/categories` — the response is identical, but now the table definitions
are built on the first request and reused forever after.

```admonish info title="When the cached table needs narrowing"
Most operations work on `&Table<...>`, but the builder methods that add conditions — like
`with_condition`, `with_search`, `with_pagination`, and `with_order` — consume `self` and
return a new `Table`. If a handler needs to narrow the cached definition, it clones first:

~~~rust
let narrowed = Category::table(db())
    .clone()                      // owned copy of the cached definition
    .with_condition(...)           // now we can narrow it
~~~

That clone is what chapter 2 meant by "cloning a table clones the definition, not the data":
it's a copy of the shape (columns, conditions, relationships), and it's cheap.
```

---

## Products of a category

The `with_many` relationship we kept from chapter 2 lets us serve a nested route —
`/categories/{id}/products`. Before writing the handler, give the relationship a proper name with an
extension trait on `Table<SqliteDB, Category>`. Add this to `src/category.rs`:

```rust
pub trait CategoryTable {
    fn ref_products(&self) -> Table<SqliteDB, Product>;
}

impl CategoryTable for Table<SqliteDB, Category> {
    fn ref_products(&self) -> Table<SqliteDB, Product> {
        self.get_ref_as("products").unwrap()
    }
}
```

Chapter 2 introduced this pattern. The trait gives the relationship a typed, discoverable name —
`ref_products()` — so call sites stop carrying the `"products"` string and the `::<Product>`
turbofish around. The `.unwrap()` is safe here because we're the ones who registered `"products"`; a
typo surfaces immediately at startup.

Now the handler. Take the cached category table, narrow it to a single id, and traverse the
relationship:

```rust
use axum::extract::Path;
use category::CategoryTable;
use product::Product;

async fn list_category_products(Path(id): Path<i64>) -> Json<Vec<Product>> {
    let id_col = Column::<i64>::new("id");
    let products = Category::table(db())
        .clone()
        .with_condition(id_col.eq(id))
        .ref_products();
    let rows = products.list().await.unwrap();
    Json(rows.into_values().collect())
}
```

Three things going on:

- **`Category::table(db()).clone()`** — we need an owned `Table` to chain `with_condition` onto, so
  we clone the cached definition. The clone copies the shape (columns, conditions, relationships),
  not any rows.
- **`with_condition(id_col.eq(id))`** — narrows the category table to one row: the one we're asking
  for. Nothing hits the database yet.
- **`ref_products()`** — traverses the `with_many` relationship we registered on Category in
  chapter 2. The returned table is `Table<SqliteDB, Product>`, already scoped to products whose
  `category_id` matches the narrowed category set. Chapter 2 walked through what the emitted SQL
  looks like; it's the same here.

Register the route in `main.rs`:

```rust
let app = Router::new()
    .route("/categories", get(list_categories))
    .route("/categories/{id}/products", get(list_category_products));
```

Hit it:

```sh
curl http://localhost:3001/categories/1/products
```

```json
[
  { "name": "Cupcake", "price": 120, "category_id": "1", "is_deleted": false },
  { "name": "Doughnut", "price": 135, "category_id": "1", "is_deleted": false },
  { "name": "Cookies", "price": 199, "category_id": "1", "is_deleted": false }
]
```

```sh
curl http://localhost:3001/categories/2/products
```

```json
[
  { "name": "Tart", "price": 220, "category_id": "2", "is_deleted": false },
  { "name": "Pie", "price": 299, "category_id": "2", "is_deleted": false }
]
```

Swap the id in the URL and the products narrow accordingly. The relationship was declared once, back
in chapter 2, and we haven't touched it since — every new nested route reuses the same declaration.

---

## A generic `crud` helper

Two handlers so far — `list_categories` and `list_category_products` — and they're already doing the
same thing: take a narrowed table, call `.list()`, return JSON. Adding `POST /categories`,
`PATCH /categories/{id}`, and `DELETE /categories/{id}` would mean four more near-identical handlers
per entity. That's not how you scale a codebase.

The five HTTP methods of CRUD all map onto one of two operation shapes:

- **List-level** at `/collection`: `GET` (list all) and `POST` (create one).
- **Item-level** at `/collection/{id}`: `GET` (read one), `PATCH` (update), `DELETE` (remove).

If we can describe the _set of rows this endpoint operates on_ with one closure, the same handler
bodies serve every entity. That closure is `Fn(SqliteDB, &Params) -> Table<SqliteDB, E>` — given the
database and whatever path params axum extracted, return the `Table` to act on. Put it in `main.rs`:

```rust
use std::collections::HashMap;
use std::sync::Arc;

type Params = HashMap<String, String>;

fn crud<E, F>(make_table: F) -> Router
where
    F: Fn(SqliteDB, &Params) -> Table<SqliteDB, E> + Send + Sync + 'static,
    E: Entity<AnySqliteType> + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    let f = Arc::new(make_table);
    Router::new()
        .route(
            "/",
            get({
                let f = f.clone();
                move |p: Option<Path<Params>>| async move {
                    let params = p.map(|Path(p)| p).unwrap_or_default();
                    let rows = f(db(), &params).list().await.unwrap();
                    Json::<Vec<E>>(rows.into_values().collect())
                }
            })
            .post({
                let f = f.clone();
                move |p: Option<Path<Params>>, Json(entity): Json<E>| async move {
                    let params = p.map(|Path(p)| p).unwrap_or_default();
                    let id = f(db(), &params).insert_return_id(&entity).await.unwrap();
                    Json(serde_json::json!({ "id": id }))
                }
            }),
        )
        .route(
            "/{id}",
            get({
                let f = f.clone();
                move |Path(params): Path<Params>| async move {
                    let id = params["id"].clone();
                    let entity = f(db(), &params).get(&id).await.unwrap();
                    Json(entity)
                }
            })
            .patch({
                let f = f.clone();
                move |Path(params): Path<Params>, Json(partial): Json<E>| async move {
                    let id = params["id"].clone();
                    let updated = f(db(), &params).patch(&id, &partial).await.unwrap();
                    Json(updated)
                }
            })
            .delete({
                let f = f;
                move |Path(params): Path<Params>| async move {
                    let id = params["id"].clone();
                    WritableDataSet::<E>::delete(&f(db(), &params), &id).await.unwrap();
                    StatusCode::NO_CONTENT
                }
            }),
        )
}
```

That's the whole thing. Five handler bodies, each tiny, all generic over the entity type. Mount
`crud(...)` under a prefix with `.nest(...)` and every route below it gets the full CRUD verb set
for free.

`/categories` becomes a one-liner:

```rust
.nest("/categories", crud(|db, _| Category::table(db).clone()))
```

That gives us:

| Method | Path             | Does                  |
| ------ | ---------------- | --------------------- |
| GET    | /categories      | list all              |
| POST   | /categories      | insert, return new id |
| GET    | /categories/{id} | fetch one             |
| PATCH  | /categories/{id} | partial update        |
| DELETE | /categories/{id} | remove                |

The nested `/categories/{cat_id}/products` route uses the same helper. The closure reads `cat_id`
out of the params map to narrow the set:

```rust
.nest(
    "/categories/{cat_id}/products",
    crud(|db, p| {
        let cat_id: i64 = p["cat_id"].parse().unwrap();
        let mut c = Category::table(db).clone();
        c.add_condition(c.id().eq(cat_id));
        c.ref_products()
    }),
)
```

`c.id()` comes from extending the `CategoryTable` trait we already set up — add it next to
`ref_products`:

```rust
pub trait CategoryTable {
    fn id(&self) -> Column<i64>;
    fn ref_products(&self) -> Table<SqliteDB, Product>;
}

impl CategoryTable for Table<SqliteDB, Category> {
    fn id(&self) -> Column<i64> {
        self.get_column("id").unwrap()
    }
    fn ref_products(&self) -> Table<SqliteDB, Product> {
        self.get_ref_as("products").unwrap()
    }
}
```

Nesting `crud` under `/categories/{cat_id}/products` gives a full CRUD surface for _products
belonging to that category_:

| Method | Path                               | Does           |
| ------ | ---------------------------------- | -------------- |
| GET    | /categories/{cat_id}/products      | list           |
| POST   | /categories/{cat_id}/products      | create         |
| GET    | /categories/{cat_id}/products/{id} | fetch one      |
| PATCH  | /categories/{cat_id}/products/{id} | partial update |
| DELETE | /categories/{cat_id}/products/{id} | remove         |

Try it:

```sh
curl -X POST http://localhost:3001/categories \
  -H 'content-type: application/json' -d '{"name":"Gluten-Free"}'
# {"id":"4"}

curl http://localhost:3001/categories/1/products/1
# {"name":"Cupcake","price":120,"category_id":"1","is_deleted":false}

curl -X PATCH http://localhost:3001/categories/1 \
  -H 'content-type: application/json' -d '{"name":"Sweet Things"}'
# {"name":"Sweet Things"}

curl -X DELETE http://localhost:3001/categories/4
# HTTP/1.1 204 No Content
```

The inline `list_categories` and `list_category_products` functions can be deleted — `crud` covers
them both.

```admonish info title="Why a HashMap for path params?"
Axum only lets a handler run the `Path` extractor **once** per request — after that, the URL
params are considered consumed. We need the outer `{cat_id}` inside the closure *and* the inner
`{id}` to identify which record to fetch, which rules out calling `Path<i64>` plus a second
`Path<String>`. Grabbing all params in one shot as `HashMap<String, String>` sidesteps the
limit — the closure picks out what it needs by name, and the item-level handlers pull `id`
from the same map.

The cost is a small amount of stringly-typed parsing (`p["cat_id"].parse::<i64>()`), which
is a fair trade for letting one `crud` function cover every route shape in the server.
```

```admonish info title="What's inside crud(), briefly"
Each HTTP method gets its own closure in the `Router`. They all need to share the same
`make_table` function, but Rust closures that capture by move can't be cloned by default, and
each axum handler is an independent `Fn` — so we wrap `make_table` in an `Arc` once and let
each handler clone the `Arc` (cheap — just a refcount bump). Inside the async block, the
closure can then invoke `f(db(), &params)` to build the narrowed table for that request.
```

---

## Error handling

Every handler in `crud` still ends in `.unwrap()`. The happy path has been fine to demo, but
an API that panics on the slightest database hiccup isn't usable. The worst part isn't the
500 — it's what happens on the wire when axum's request task panics: the connection is
dropped and the client sees an empty reply, with no status code and no body to explain.

A proper REST API needs three things:

- Missing resources return **404**, not 500.
- Bad JSON bodies return **400** with a useful message.
- Everything else returns **500** but with a structured JSON body, not silence.

Axum already gives us the middle one for free — it rejects malformed `Json<E>` bodies with 400.
The other two come down to converting `VantageError` into an HTTP response. Add an `ApiError`
type to `main.rs`:

```rust
use axum::response::{IntoResponse, Response};

struct ApiError {
    status: StatusCode,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

impl From<VantageError> for ApiError {
    fn from(e: VantageError) -> Self {
        let message = e.to_string();
        let status = if message.contains("no row found") {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        eprintln!("API error: {:?}", e);
        Self { status, message }
    }
}

type ApiResult<T> = Result<T, ApiError>;
```

Two things earn their keep:

- **`IntoResponse`** makes `ApiError` returnable from a handler; axum calls `into_response()`
  to assemble status, headers, and body.
- **`From<VantageError>`** lets us use `?` inside a handler. Every `.await?` now short-circuits
  to an `ApiError` that axum will render for us.

With that in place, the `unwrap`s inside `crud` turn into `?`, and each handler returns
`ApiResult<T>`:

```rust
get({
    let f = f.clone();
    move |p: Option<Path<Params>>| async move {
        let params = p.map(|Path(p)| p).unwrap_or_default();
        let rows = f(db(), &params).list().await?;
        ApiResult::Ok(Json::<Vec<E>>(rows.into_values().collect()))
    }
})
```

Do the same for the other four (`post`, `get /{id}`, `patch`, `delete`) — drop the
`.unwrap()`, add `?`, wrap the happy path in `ApiResult::Ok(...)`. Handler bodies stay three
lines long.

Try a few error cases:

```sh
curl -w "\nstatus=%{http_code}\n" http://localhost:3001/categories/999
# {"error":"get_table_value: no row found (id: \"999\")"}
# status=404

curl -w "\nstatus=%{http_code}\n" -X PATCH http://localhost:3001/categories/999 \
  -H 'content-type: application/json' -d '{"name":"Ghost"}'
# {"error":"get_table_value: no row found (id: \"999\")"}
# status=404

curl -w "\nstatus=%{http_code}\n" -X POST http://localhost:3001/categories \
  -H 'content-type: application/json' -d '{not-json}'
# Failed to parse the request body as JSON: ...
# status=400

curl -w "\nstatus=%{http_code}\n" -X DELETE http://localhost:3001/categories/999
# status=204
```

Missing ids produce 404s with a JSON body the client can show to a user. The malformed body
gets axum's built-in 400 for free. `DELETE` on a missing id still returns 204 — vantage's
`delete` is idempotent, and "the resource is gone" is true whether or not it was ever there.

```admonish info title="Why match on the error message?"
`VantageError` is a flat struct — it has a message, a location, and a context map, but no
typed discriminant you could match against. "Not-found" errors happen to carry the string
`"no row found"` in their message, so that's what we key on.

It's slightly brittle: if vantage ever rewrites that message, every API using this pattern
silently downgrades 404s to 500s. A stronger guarantee would require vantage exposing an
`ErrorKind` enum, or for the handler to detect missing rows explicitly — narrow the table by
id and call `get_some()`, where `None` means absent. For this tutorial the string match is
good enough; production code with strict SLAs should prefer the explicit `get_some` path.
```

```admonish info title="Logging with {:?} on the server side"
`eprintln!("API error: {:?}", e);` prints the full error structure — location, context,
nested sources — which is exactly what a human debugging a 500 needs. The client meanwhile
only sees `e.to_string()`, the short one-liner message. That's the whole point of
`.context("…")` from chapter 1: context accumulates on the server, a single sentence reaches
the client.

Swap `eprintln!` for `tracing::error!` if you're wired up for structured logging. The
mechanics are identical.
```

---

## Pagination and search

Three categories is fine for dev; three thousand would crush any client that naively calls
`GET /categories` and tries to render everything. Real APIs page through long lists and let
callers filter by a search term — two query-string features that belong inside `crud` so
every entity gets them.

Axum parses query strings for us via the `Query<T>` extractor. Add a small struct for the
parameters, and a second extractor on the list handler:

```rust
use axum::extract::Query;
use vantage_table::pagination::Pagination;

#[derive(Deserialize, Default)]
struct ListQuery {
    page: Option<i64>,
    per_page: Option<i64>,
    q: Option<String>,
}
```

Inside `crud`, only the `GET /` handler changes. It takes both extractors, mutates the narrowed
table with whatever the caller asked for, and lists:

```rust
get({
    let f = f.clone();
    move |p: Option<Path<Params>>, Query(q): Query<ListQuery>| async move {
        let params = p.map(|Path(p)| p).unwrap_or_default();
        let mut t = f(db(), &params);
        if q.page.is_some() || q.per_page.is_some() {
            t.set_pagination(Some(Pagination::new(
                q.page.unwrap_or(1),
                q.per_page.unwrap_or(50),
            )));
        }
        if let Some(term) = q.q.as_deref() {
            t.add_search(term);
        }
        let rows = t.list().await?;
        ApiResult::Ok(Json::<Vec<E>>(rows.into_values().collect()))
    }
})
```

Two things happen:

- **`set_pagination(Some(Pagination::new(page, per_page)))`** takes a page number and a page
  size. Vantage applies these as `LIMIT … OFFSET …` on the SELECT. Missing params fall back
  to the defaults (page 1, 50 per page) — and if neither is supplied we don't touch pagination
  at all, so the unfiltered list still hits the whole set.
- **`add_search(term)`** is the `.with_search` we used in chapter 2 to add a LIKE filter across
  all columns. Both end up as extra `WHERE` clauses on the query that vantage already compiles
  for us.

Try it:

```sh
curl "http://localhost:3001/categories?page=1&per_page=2"
# [{"name":"Sweet Treats"},{"name":"Pastries"}]

curl "http://localhost:3001/categories?page=2&per_page=2"
# [{"name":"Breads"}]

curl "http://localhost:3001/categories?q=Pastries"
# [{"name":"Pastries"}]

curl "http://localhost:3001/categories?q=e"
# [{"name":"Sweet Treats"},{"name":"Pastries"},{"name":"Breads"}]
```

The nested route gets these for free — `crud` is the same function. `per_page` works on
`/categories/{cat_id}/products` out of the box:

```sh
curl "http://localhost:3001/categories/1/products?per_page=2"
# [
#   {"name":"Cupcake","price":120,"category_id":"1","is_deleted":false},
#   {"name":"Doughnut","price":135,"category_id":"1","is_deleted":false}
# ]
```

Because the closure for the nested mount narrows the table with `with_condition` *before*
`crud` applies its own pagination/search, the filters compose cleanly: the category scope stays
in effect, and pagination just counts rows within it.

```admonish info title="What about ordering?"
A full `?order_by=price&dir=desc` pairing is the obvious next thing — and the Table API
supports it via `add_order(column.ascending())` — but the `OrderBy` type is generic over the
backend's condition type (`T::Condition`), not the easier-to-hand-you `Expression<T::Value>`
that `get_column_expr` returns. Wiring it up at the generic `crud` level takes a small extra
layer of `From` conversions that would balloon this section.

For a single-entity handler, you can simply narrow with `let mut t = Category::table(db())
.clone(); t.add_order(sqlite_expr!("{}", ident("name")).ascending());`. Adding ordering to
`crud` is a fine exercise once the rest of the server is in place — and a natural thing to
push back into the framework so every backend picks it up uniformly.
```

```admonish info title="Validating pagination params"
Our `ListQuery` silently accepts `page=0`, `per_page=-5`, or `per_page=1000000`. `Pagination::new`
clamps page and items-per-page to at least 1, so the first two can't crash us — but an API
that hands out 1M rows because someone asked for it is a DoS target. For production,
extend `ListQuery` with a `fn validate(&self) -> Result<(), ApiError>` that caps `per_page`
at something like 200 and returns 400 otherwise. The plumbing is already there — `ApiError`
already knows how to render a 400.
```

---

## Migrating to MongoDB

Chapter 2 closed on a claim: the model layer isolates business code from storage, so swapping
databases is a change to the model, not to the routes, handlers, or business logic. Now we
cash the check. `/categories`, `/categories/{id}`, and `/categories/{cat_id}/products` keep
their exact URL shape, response bodies, and behaviour — but the data lives in MongoDB instead
of SQLite.

Start Mongo — Docker is the easiest way:

```sh
docker run -d --name mongo-learn -p 27017:27017 mongo:7
```

### Cargo.toml

Drop `vantage-sql`, add `vantage-mongodb`:

```toml
[dependencies]
axum = "0.8.9"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
vantage-core = { path = "../vantage-core" }
vantage-dataset = { path = "../vantage-dataset" }
vantage-expressions = { path = "../vantage-expressions" }
vantage-mongodb = { path = "../vantage-mongodb" }
vantage-table = { path = "../vantage-table" }
vantage-types = { path = "../vantage-types", features = ["serde"] }
```

### Entities

Swap the `#[entity]` type tag from `SqliteType` to `MongoType` and the id column name from
`id` to the MongoDB-idiomatic `_id`. Imports collapse to a single prelude use-line.

`src/category.rs`:

```rust
use std::sync::OnceLock;

use vantage_mongodb::prelude::*;

use crate::product::Product;

#[entity(MongoType)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
}

impl Category {
    pub fn table(db: MongoDB) -> &'static Table<MongoDB, Category> {
        static CACHE: OnceLock<Table<MongoDB, Category>> = OnceLock::new();
        CACHE.get_or_init(|| {
            Table::new("category", db)
                .with_id_column("_id")
                .with_column_of::<String>("name")
                .with_many("products", "category_id", |db| Product::table(db).clone())
        })
    }
}
```

`src/product.rs` gets the symmetric changes:

```rust
use std::sync::OnceLock;

use vantage_mongodb::prelude::*;

use crate::category::Category;

#[entity(MongoType)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Product {
    pub name: String,
    pub price: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub is_deleted: bool,
}

impl Product {
    pub fn table(db: MongoDB) -> &'static Table<MongoDB, Product> {
        static CACHE: OnceLock<Table<MongoDB, Product>> = OnceLock::new();
        CACHE.get_or_init(|| {
            let is_deleted = Column::<bool>::new("is_deleted");
            Table::new("product", db)
                .with_id_column("_id")
                .with_column_of::<String>("name")
                .with_column_of::<i64>("price")
                .with_column_of::<String>("category_id")
                .with_column_of::<bool>("is_deleted")
                .with_condition(is_deleted.eq(false))
                .with_one("category", "category_id", |db| Category::table(db).clone())
        })
    }
}
```

Nothing about the struct shape changed — `category_id` and `is_deleted` have been real columns
since § 2. The `with_*` calls line up one-for-one with the SQLite version; only the id column
name and the entity's type tag shift.

The `CategoryTable` trait from § 3 also updates — MongoDB's `_id` is a string, not an integer:

```rust
impl CategoryTable for Table<MongoDB, Category> {
    fn id(&self) -> Column<String> {
        self.get_column("_id").unwrap()
    }
    fn ref_products(&self) -> Table<MongoDB, Product> {
        self.get_ref_as("products").unwrap()
    }
}
```

The nested route's closure gets a little simpler as a result — no parse step, just narrow by the
URL's `cat_id` string directly:

```rust
crud(|db, p| {
    let mut c = Category::table(db).clone();
    c.add_condition(c.id().eq(p["cat_id"].as_str()));
    c.ref_products()
})
```

`.eq(&str)` works here because `vantage-mongodb`'s `From<&str> for AnyMongoType` auto-promotes
24-character hex strings to `ObjectId` and leaves everything else as `String`. The comparison fires
with the right BSON type whichever `_id` convention the collection uses.

### main.rs

The `crud` function's generic bounds shift from `Entity<AnySqliteType>` to `Entity<AnyMongoType>`
and its `Fn(SqliteDB, ...)` becomes `Fn(MongoDB, ...)`. Body unchanged.

```rust
fn crud<E, F>(make_table: F) -> Router
where
    F: Fn(MongoDB, &Params) -> Table<MongoDB, E> + Send + Sync + 'static,
    E: Entity<AnyMongoType> + Serialize + DeserializeOwned + Send + Sync + 'static,
```

Connection, id handling, and error mapping are the only handler-level changes:

```rust
// db() returns MongoDB; connect with URL + database name.
let conn = MongoDB::connect("mongodb://localhost:27017", "learn3")
    .await
    .context("Failed to connect to MongoDB")?;

// item-level ids are MongoId. String → MongoId dispatches to ObjectId when the
// string is a 24-char hex, otherwise stays a plain String, via a `From<String>`
// smart-parse in vantage-mongodb.
let id: MongoId = params["id"].clone().into();

// MongoDB's missing-doc error joins the 404 path.
let status = if message.contains("no row found") || message.contains("Document not found") {
    StatusCode::NOT_FOUND
} else {
    StatusCode::INTERNAL_SERVER_ERROR
};
```

### Running it

```sh
cargo run
```

The collection is empty, so the first request returns an empty array:

```sh
curl http://localhost:3001/categories
# []
```

POST a few categories — responses carry the auto-generated MongoDB ObjectId as the new id:

```sh
SWEETS=$(curl -s -X POST http://localhost:3001/categories \
  -H 'content-type: application/json' -d '{"name":"Sweet Treats"}' \
  | jq -r .id)
echo "$SWEETS"
# 69e2b9101a552c206f5f8468
```

Create a product in that category by POSTing to the nested route, including the parent id as
`category_id` in the body:

```sh
curl -X POST "http://localhost:3001/categories/$SWEETS/products" \
  -H 'content-type: application/json' \
  -d "{\"name\":\"Cupcake\",\"price\":120,\"category_id\":\"$SWEETS\"}"
# {"id":"69e2b9101a552c206f5f846a"}

curl "http://localhost:3001/categories/$SWEETS/products"
# [{"name":"Cupcake","price":120,"category_id":"69e2b9101a552c206f5f8468","is_deleted":false}]
```

Same URL shape as the SQLite version. Same JSON. Same error codes. Handlers, routing,
pagination, filtering — nothing in the request path learned that storage moved from a local
file to a document database.

```admonish info title="Cross-type $in in relationship traversal"
`CategoryTable::ref_products()` still works. MongoDB's `_id` defaults to `ObjectId`, but
application fields like `product.category_id` arrive as plain JSON strings and get stored as
BSON `String`. A naive `$in: [ObjectId(...)]` wouldn't match. `vantage-mongodb` sidesteps that
by pushing both representations into the `$in` inside `related_in_condition` — an `ObjectId`
value also emits its 24-char hex string, and a hex-shaped `String` value also emits the
parsed `ObjectId`. Traversal works regardless of which form the target stores.
```

```admonish info title="String _ids are also an option"
Nothing in MongoDB requires `_id` to be an `ObjectId` — it can be any BSON value, including a
plain string. If the app supplies `_id` explicitly on insert (or the framework generates a
UUID and writes it into `_id`), both `category._id` and `product.category_id` are strings and
everything lines up without the $in dual-push. Useful when you want stable, human-legible ids
or ids that came from an upstream system.
```

---

## Scaling up: CRUD as a one-liner

The `crud` function, `ApiError`, `Params`, and `ListQuery` aren't really tied to this app —
they're generic over any `Table<MongoDB, E>`. Move them into their own module and `main.rs`
collapses to exactly what it's about: connecting the database and registering routes. Three
files do the work:

**`src/vantage_axum.rs`** (one file, ~120 lines) — everything HTTP: `ApiError`, `ListQuery`,
and the `crud<E, F>` helper. Accepts any entity `E` that implements `Entity<AnyMongoType>`
plus the usual serde bounds.

**`src/db.rs`** (17 lines) — the `static DB: OnceLock<MongoDB>`, an `init(url, db)` helper
that connects and stores the handle, and a `pub fn db() -> MongoDB` accessor that hands out
cheap `MongoDB` clones.

**`src/main.rs`** — now down to ~30 lines:

```rust
mod category;
mod db;
mod product;
mod vantage_axum;

use axum::Router;
use category::{Category, CategoryTable};
use vantage_axum::crud;
use vantage_mongodb::prelude::*;

#[tokio::main]
async fn main() -> VantageResult<()> {
    db::init("mongodb://localhost:27017", "learn3").await?;

    let app = Router::new()
        .nest("/categories", crud(|db, _| Category::table(db).clone()))
        .nest(
            "/categories/{cat_id}/products",
            crud(|db, p| {
                let mut c = Category::table(db).clone();
                c.add_condition(c.id().eq(p["cat_id"].as_str()));
                c.ref_products()
            }),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}
```

Adding a new entity to this server is now *two* things:

1. Write a `Table::new(...)` constructor for it — declarative, one function, same shape we
   learned in chapter 2.
2. Mount it.

```rust
.nest("/widgets", crud(|db, _| Widget::table(db).clone()))
```

That's the whole surface. `GET` list, `POST` create, `GET /{id}`, `PATCH /{id}`, `DELETE
/{id}`, pagination via `?page=&per_page=`, full-text search via `?q=`, 404s for missing ids,
400s for malformed bodies, structured JSON errors. No new handler code. No per-entity error
mapping. No per-entity query-param struct. One line per entity.

Every route plays by the same rules because every route is served by the same `crud` — the
single description of "what this route does" lives in the entity's `Table` definition, and
the HTTP boundary just routes to it. That is the "one description, many operations"
principle from chapter 2's `Table` carried all the way to the wire, unbroken.

The `vantage_axum` module is generic enough to lift directly into a larger codebase — it has
no knowledge of `Category`, `Product`, or your particular routes. Drop it into your own
binary, give it a `db()` accessor, write entity files, mount routes.
