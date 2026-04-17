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

# Reference data, served from memory
curl "http://localhost:3001/countries/FR"

# Writes
curl -X POST "http://localhost:3001/categories" \
  -H 'content-type: application/json' -d '{"name":"Gluten-Free"}'
```

A few things about the shape of this API:

- `/categories/{id}/products` is a **nested** route — products narrowed by the relationship we
  defined in chapter 2. The handler for it is the same generic `list` as `/products`, just applied
  to a scoped table.
- `/countries` is backed by **`vantage-redb`**, an in-memory backend seeded from a CSV at startup.
  Reference data that rarely changes doesn't need a round-trip to disk on every request.
- `/products` and `/categories` start out on SQLite, same as chapter 2. Toward the end of the
  chapter we **migrate** them to MongoDB. That migration is a change to the model file; the handlers
  and routes don't know the difference.

The handler functions are each written **once** and mounted against any `Table<Backend, Entity>` the
router has on hand — one `list`, one `get`, one `post`, one `patch`, one `delete`, parameterised
over the backend and entity types. Adding another entity is a route registration, not a new handler.

---

## The minimum Axum skeleton

We keep `product.rs` and `category.rs` from chapter 2 and replace `main.rs` with an Axum server. One
new dependency:

```sh
cargo add axum
```

Tokio and serde are already pulled in from earlier chapters. `axum` is the HTTP framework; it uses
the existing `serde` to encode response bodies as JSON.

Axum needs `Category` to be serialisable, so add `Serialize` and `Deserialize` to its derive list —
the first for GET responses, the second for POST bodies we'll introduce shortly. Also drop the
computed `title` field and its expressions from chapter 2: computed fields don't round-trip through
a POST cleanly (the JSON body would try to write a column that doesn't exist), and we'll bring them
back in a later chapter that handles writable computed fields properly.

In `src/category.rs`:

```rust
use serde::{Deserialize, Serialize};

#[entity(SqliteType)]
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
}
```

Comment out the two `with_expression` calls in `Category::table` too. `Product::table`'s `category`
expression was pulling `title` from the category table; switch it to pull `name` instead:

```rust
.with_expression("category", |t| {
    t.get_subquery_as::<Category>("category")
        .unwrap()
        .select_column("name")
})
```

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
- **Each request calls `Category::table(db())`** — building the table (columns, relationships,
  expressions from chapter 2) — and then lists it.
- **The handler returns `Json<Vec<Category>>` directly.** No DTO layer. Whatever the entity
  contains, including the computed `title` expression from chapter 2, comes out in the JSON.

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
  { "name": "Sweet Treats", "title": "Sweet Treats (3)" },
  { "name": "Pastries", "title": "Pastries (2)" },
  { "name": "Breads", "title": "Breads (1)" }
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
                // ...columns and expressions unchanged...
                .with_many("products", "category_id", |db| Product::table(db).clone())
                // ...
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
  we clone the cached definition. The clone copies the shape (columns, relationships, expressions),
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
  { "name": "Cupcake", "price": 120, "category": "Sweet Treats" },
  { "name": "Doughnut", "price": 135, "category": "Sweet Treats" },
  { "name": "Cookies", "price": 199, "category": "Sweet Treats" }
]
```

```sh
curl http://localhost:3001/categories/2/products
```

```json
[
  { "name": "Tart", "price": 220, "category": "Pastries" },
  { "name": "Pie", "price": 299, "category": "Pastries" }
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
# {"name":"Cupcake","price":120}

curl -X PATCH http://localhost:3001/categories/1 \
  -H 'content-type: application/json' -d '{"name":"Sweet Things"}'
# {"name":"Sweet Things"}

curl -X DELETE http://localhost:3001/categories/4
# HTTP/1.1 204 No Content
```

The inline `list_categories` and `list_category_products` functions can be deleted — `crud` covers
them both.

Product needs the same treatment as Category did earlier: add `Deserialize` to its derives and drop
the computed `category` field (and its `with_expression`) so POST bodies round-trip cleanly. After
this, a product is simply `{ name, price }`.

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
# [{"name":"Cupcake","price":120},{"name":"Doughnut","price":135}]
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
