# Step 2: Make Expressions Work

With the type system in place, you can now use `Expression<AnySqliteType>` to build and execute
queries. This step has two deliverables: a convenience macro and the `ExprDataSource` trait
implementation.

### The vendor macro

Define a macro that produces `Expression<YourAnyType>`. SurrealDB has `surreal_expr!`, we create
`sqlite_expr!`:

```rust
# use vantage_sql::sqlite_expr;
let expr = sqlite_expr!("SELECT * FROM product WHERE price > {}", 100i64);
```

Under the hood, `100i64` gets wrapped as `AnySqliteType::new(100i64)` with variant `Integer`. When
this expression hits the database, the bind layer knows to call `query.bind(100i64)` — not
`query.bind("100")` or `query.bind(100.0)`.

The macro handles three kinds of parameters:

- `42i64` → scalar with type marker
- `(sub_expr)` → nested expression (composed into the template)
- `{deferred}` → lazy evaluation (resolved at execution time)

### Identifier quoting

SQL identifiers (table names, column names) need quoting to handle reserved words, spaces, and
special characters. Different databases use different quote styles — PostgreSQL and SQLite use
double quotes (`"name"`), MySQL uses backticks (`` `name` ``), SurrealDB uses something else
entirely.

Vantage centralises this in the `Identifier` struct (`vantage-sql/src/primitives/identifier.rs`).
`Identifier` is quote-agnostic — it stores the name parts and optional alias, but the actual quoting
happens in the `Expressive<T>` implementation for each backend type:

```text
impl Expressive<AnyMysqlType> for Identifier {
    fn expr(&self) -> Expression<AnyMysqlType> {
        Expression::new(self.render_with('`'), vec![])  // `name`
    }
}
```

When you add a new SQL backend, add an `Expressive<YourAnyType>` impl with your quote character. The
compiler picks the right impl based on the expression type context.

In practice you use the `ident()` shorthand and pass it into the vendor macro with parentheses — the
`(...)` syntax calls `.expr()` automatically, so quoting is handled by the type context:

```rust
# use vantage_sql::{mysql_expr, postgres_expr};
use vantage_sql::primitives::identifier::{Identifier, ident};

// The (ident(...)) syntax invokes Expressive — quoting is automatic
let expr = mysql_expr!("SELECT {} FROM {} WHERE {} = {}",
    (ident("name")), (ident("product")), (ident("price")), 100i64);
// → SELECT `name` FROM `product` WHERE `price` = 100

let expr = postgres_expr!("SELECT {} FROM {} WHERE {} = {}",
    (ident("name")), (ident("product")), (ident("price")), 100i64);
// → SELECT "name" FROM "product" WHERE "price" = 100
```

For qualified identifiers (table.column) and aliases:

```rust
# use vantage_sql::primitives::identifier::{Identifier, ident};
# use vantage_sql::{mysql_expr, sqlite_expr};
let expr = sqlite_expr!("SELECT {}", (ident("name").dot_of("u")));
// → SELECT "u"."name"

let expr = mysql_expr!("SELECT {}", (ident("name").with_alias("n")));
// → SELECT `name` AS `n`
```

Test identifier quoting in `tests/<backend>/2_identifier.rs` — cover basic names, reserved words,
spaces, hyphens, unicode, and names that start with numbers. These are all legal inside quoted
identifiers in both PostgreSQL and MySQL.

### ExprDataSource

Implement `DataSource` (marker) and `ExprDataSource<AnySqliteType>` on your DB struct. The `execute`
method takes an expression, flattens nested sub-expressions, converts `{}` placeholders to your
driver's syntax (`?N` for SQLite, `$N` for Postgres), binds parameters using type markers, and
returns results.

Results come back as `AnySqliteType` with `type_variant: None` — the database doesn't preserve our
markers, so results are permissive (see Step 1). For SQLite that's especially natural since it
doesn't distinguish boolean from integer on the wire.

If the persistence layer you're implementing _does_ preserve type information in responses (like
SurrealDB with CBOR tags), set the correct `type_variant` when constructing result values in your
`execute()` implementation. That way `try_get` enforces type boundaries on both sides of the
round-trip.

### Validating with INSERT expressions

The best way to test this is INSERT + SELECT round-trips. A single insert exercises all the pieces —
macro, parameter binding, type markers, and result parsing:

```rust
# use vantage_expressions::ExprDataSource;
# use vantage_sql::sqlite::SqliteDB;
# use vantage_sql::sqlite_expr;
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
# db.execute(&sqlite_expr!(
#     "CREATE TABLE product (id TEXT PRIMARY KEY, name TEXT, price INTEGER, is_deleted BOOLEAN)"
# ))
# .await?;
let insert = sqlite_expr!(
    "INSERT INTO product (id, name, price, is_deleted) VALUES ({}, {}, {}, {})",
    "cupcake", "Flux Cupcake", 120i64, false
);
db.execute(&insert).await?;

let select = sqlite_expr!("SELECT * FROM product WHERE id = {}", "cupcake");
let result = db.execute(&select).await?;
# Ok(())
# }
```

Nested expressions let you build multi-row inserts from composable parts:

```rust
# use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
# use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
# use vantage_sql::sqlite_expr;
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
# db.execute(&sqlite_expr!(
#     "CREATE TABLE product (id TEXT PRIMARY KEY, name TEXT, price INTEGER, is_deleted BOOLEAN)"
# ))
# .await?;
let row1 = sqlite_expr!("({}, {}, {}, {})", "tart", "Time Tart", 220i64, false);
let row2 = sqlite_expr!("({}, {}, {}, {})", "pie", "Sea Pie", 299i64, true);

// Expression::from_vec joins sub-expressions with a delimiter
let rows = Expression::from_vec(vec![row1, row2], ", ");

// Nest into the INSERT — flattener resolves everything into a single query
let insert = Expression::<AnySqliteType>::new(
    "INSERT INTO product (id, name, price, is_deleted) VALUES {}",
    vec![ExpressiveEnum::Nested(rows)],
);
db.execute(&insert).await?;
# let count = db.execute(&sqlite_expr!("SELECT COUNT(*) FROM product")).await?;
# assert_eq!(count.unwrap_scalar().try_get::<i64>(), Some(2));
# Ok(())
# }
```

The `ExpressionFlattener` collapses all nesting into one flat template with positional parameters —
each one still carrying its type marker for correct binding.

### Deferring: cross-database value resolution

Sometimes a query on one database needs a value from another database. That's what `defer()` is for
— it wraps a query as a closure that executes later, when the outer query runs.

This is not a subquery. The deferred query runs first, produces a concrete value, and that value
gets bound as a regular parameter in the outer query.

```rust
# use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
# use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
# use vantage_sql::sqlite_expr;
# async fn setup() -> (SqliteDB, SqliteDB) {
#     let config_db = SqliteDB::connect("sqlite::memory:").await.unwrap();
#     for sql in [
#         "CREATE TABLE config (key TEXT PRIMARY KEY, value INTEGER)",
#         "INSERT INTO config VALUES ('min_price', 150)",
#     ] {
#         config_db.execute(&sqlite_expr!(sql)).await.unwrap();
#     }
#     let shop_db = SqliteDB::connect("sqlite::memory:").await.unwrap();
#     for sql in [
#         "CREATE TABLE product (name TEXT, price INTEGER)",
#         "INSERT INTO product VALUES ('Cheap', 50), ('Tart', 220), ('Pie', 299)",
#     ] {
#         shop_db.execute(&sqlite_expr!(sql)).await.unwrap();
#     }
#     (config_db, shop_db)
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
let (config_db, shop_db) = setup().await;

// This doesn't execute yet — it's a closure
let threshold_query = sqlite_expr!("SELECT value FROM config WHERE key = {}", "min_price");
let deferred_threshold = config_db.defer(threshold_query);

// Use the deferred value as a parameter in a different database
let shop_query = Expression::<AnySqliteType>::new(
    "SELECT name FROM product WHERE price >= {} ORDER BY price",
    vec![ExpressiveEnum::Deferred(deferred_threshold)],
);

// When shop_db.execute() runs:
// 1. Resolves the deferred → calls config_db, gets 150
// 2. Replaces the Deferred param with Scalar(150)
// 3. Flattens and binds: SELECT name FROM product WHERE price >= ?1
let result = shop_db.execute(&shop_query).await?;
# Ok(())
# }
```

Your `execute()` implementation needs to resolve deferred parameters before flattening. Walk the
parameter list, call `.call().await` on any `Deferred`, and leave `Scalar` and `Nested` untouched.

The resolved value comes back as an untyped `AnySqliteType` (no variant marker), so it gets bound
via JSON-inference. For SQLite this is fine — the loose type system handles it. For stricter
databases, you may want `defer()` to preserve type information from the source query's result.

### Reading query results

So far we've been calling `db.execute(&expr).await` which returns `AnySqliteType`. For a SELECT
query, that value wraps a JSON array of row objects. To work with individual rows, you convert into
Records:

```rust
# use serde_json::Value as JsonValue;
# use vantage_expressions::ExprDataSource;
# use vantage_sql::sqlite::SqliteDB;
# use vantage_sql::sqlite_expr;
# use vantage_types::Record;
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
# for sql in [
#     "CREATE TABLE product (id TEXT PRIMARY KEY, name TEXT, price INTEGER)",
#     "INSERT INTO product VALUES ('a', 'Cheap', 50), ('b', 'Tart', 220), ('c', 'Pie', 299)",
# ] {
#     db.execute(&sqlite_expr!(sql)).await?;
# }
let result = db.execute(&sqlite_expr!("SELECT * FROM product")).await?;

// Result is AnySqliteType wrapping [{"id":"a","name":"Cheap","price":50}, ...]
// Convert to records manually:
let rows: Vec<ciborium::Value> = match result.into_value() {
    ciborium::Value::Array(arr) => arr,
    _ => panic!("expected rows"),
};
let record: Record<ciborium::Value> = rows[0].clone().into();
# Ok(())
# }
```

That works but it's verbose. The `TryFrom<AnyType>` impls from Step 1 make this cleaner through
`AssociatedExpression`. When you call `db.associate::<R>(expr)`, you get an expression that knows
its return type — `.get()` executes and converts in one step:

```rust
# use serde_json::Value as JsonValue;
# use vantage_expressions::ExprDataSource;
# use vantage_sql::sqlite::SqliteDB;
# use vantage_sql::sqlite_expr;
# use vantage_types::Record;
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
# for sql in [
#     "CREATE TABLE product (id TEXT PRIMARY KEY, name TEXT, price INTEGER)",
#     "INSERT INTO product VALUES ('a', 'Cheap', 50), ('b', 'Tart', 220), ('c', 'Pie', 299)",
# ] {
#     db.execute(&sqlite_expr!(sql)).await?;
# }
// Scalar — extracts single value from single-row result
let count = db.associate::<i64>(sqlite_expr!("SELECT COUNT(*) FROM product"));
assert_eq!(count.get().await?, 3);

// Record — extracts first row
let record: Record<JsonValue> = db
    .associate(sqlite_expr!("SELECT * FROM product WHERE id = {}", "c"))
    .get().await?;
# assert_eq!(record["name"], "Pie");
# Ok(())
# }
```

From a Record, you can deserialize into a struct. For the `#[entity]` path:

```rust
# use vantage_expressions::ExprDataSource;
# use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
# use vantage_sql::sqlite_expr;
# use vantage_types::{Record, TryFromRecord, entity};
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> vantage_core::Result<()> {
# let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
# for sql in [
#     "CREATE TABLE product (id TEXT PRIMARY KEY, name TEXT, price INTEGER)",
#     "INSERT INTO product VALUES ('a', 'Cheap', 50), ('b', 'Tart', 220), ('c', 'Pie', 299)",
# ] {
#     db.execute(&sqlite_expr!(sql)).await?;
# }
#[entity(SqliteType)]
struct Product { id: String, name: String, price: i64 }

let record: Record<AnySqliteType> = db
    .associate(sqlite_expr!("SELECT * FROM product WHERE id = {}", "c"))
    .get().await?;
let product = Product::from_record(record)?;
# assert_eq!(product.price, 299);
# Ok(())
# }
```

Or for the serde path with `Record<JsonValue>`:

```rust
# use serde::Deserialize;
# use serde_json::Value as JsonValue;
# use vantage_expressions::ExprDataSource;
# use vantage_sql::sqlite::SqliteDB;
# use vantage_sql::sqlite_expr;
# use vantage_types::{Record, TryFromRecord};
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let db = SqliteDB::connect("sqlite::memory:").await?;
# for sql in [
#     "CREATE TABLE product (id TEXT PRIMARY KEY, name TEXT, price INTEGER)",
#     "INSERT INTO product VALUES ('a', 'Cheap', 50), ('b', 'Tart', 220), ('c', 'Pie', 299)",
# ] {
#     db.execute(&sqlite_expr!(sql)).await?;
# }
#[derive(Deserialize)]
struct Product { id: String, name: String, price: i64 }

let record: Record<JsonValue> = db
    .associate(sqlite_expr!("SELECT * FROM product WHERE id = {}", "c"))
    .get().await?;
let product: Product = Product::from_record(record)?;
# assert_eq!(product.name, "Pie");
# Ok(())
# }
```

Testing the failure modes (missing fields, NULL into required field, wrong types) can help spot
issues in your implementation.

### Step 2 conclusion

At this point you should have:

1. **A vendor macro** (`sqlite_expr!`, `surreal_expr!`, etc.) that produces `Expression<AnyType>`
   with typed parameters.

2. **Trait impls** in `src/<backend>/impls/` — `DataSource` (marker) and `ExprDataSource<AnyType>`
   with `execute()` and `defer()`.

3. **Tests** in `tests/<backend>/2_*.rs` covering:
   - INSERT with typed parameters, read back and verify
   - Multi-row INSERT using nested expressions and `from_vec`
   - Type marker verification (bool binds as bool, not as string "true")
   - Cross-database deferred value resolution
   - AssociatedExpression with scalar, Record, and entity results
   - Identifier quoting: basic names, reserved words, spaces, hyphens, unicode

