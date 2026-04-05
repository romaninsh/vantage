# Adding a New Persistence Backend to Vantage

So you want to connect Vantage to a new database? This guide walks through the process
using SQLite as the example. The same pattern applies whether you're adding Postgres,
MongoDB, or anything else.

## Step 1: Define Your Type System

Every database has its own idea of what types exist. SQLite has 5 storage classes
(NULL, INTEGER, REAL, TEXT, BLOB). Postgres has dozens. SurrealDB has its own set
with Things and Geometry types.

The vantage type system gives you two things:

1. **Type markers** — so you can tell the difference between "this is an integer" and
   "this is text" even when both are stored as `serde_json::Value` under the hood.
2. **Safe extraction** — `try_get::<i64>()` on a text value returns `None` instead of
   silently coercing. This prevents the kind of bugs where a string "42" gets treated
   as a number somewhere downstream.

### Why not just use serde_json::Value directly?

You can! And for simple cases it works fine. The problem shows up when values move
between contexts. A JSON number `42` could be an integer, a float, or even a boolean
(SQLite stores bools as 0/1). Without type markers, you lose that distinction and
get silent data corruption.

### Setting it up

Use the `vantage_type_system!` macro. It generates a trait, an enum of variants,
and a type-erased `AnyType` wrapper:

```rust
vantage_type_system! {
    type_trait: SqliteType,
    method_name: json,
    value_type: serde_json::Value,
    type_variants: [Null, Integer, Text, Real, Numeric, Blob]
}
```

This gives you `SqliteType` (trait), `SqliteTypeVariants` (enum), and `AnySqliteType`
(the type-erased wrapper that remembers which variant a value belongs to).

The `value_type` is whatever your database driver naturally speaks. For SQL databases
that's usually `serde_json::Value`. SurrealDB uses `ciborium::Value` (CBOR). You could
use any type — even `String` if your storage is that simple.

Then implement the trait for each Rust type. Here's bool — SQLite stores it as 0/1:

```rust
impl SqliteType for bool {
    type Target = SqliteTypeIntegerMarker;  // bool lives in the Integer family

    fn to_json(&self) -> serde_json::Value {
        Value::Number(if *self { 1.into() } else { 0.into() })
    }

    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().map(|i| i != 0),
            Value::Bool(b) => Some(b),  // accept native bools too
            _ => None,
        }
    }
}
```

And here's how the type safety works in practice:

```rust
let val = AnySqliteType::new(42i64);
assert_eq!(val.try_get::<i64>(), Some(42));    // same variant → works
assert_eq!(val.try_get::<String>(), None);      // Integer ≠ Text → rejected

let val = AnySqliteType::new("hello".to_string());
assert_eq!(val.try_get::<String>(), Some("hello".to_string()));
assert_eq!(val.try_get::<i64>(), None);          // Text ≠ Integer → rejected
```

You also need `From` conversions so values can be created conveniently:

```rust
let val: AnySqliteType = 42i64.into();
let val: AnySqliteType = "hello".into();
let val: AnySqliteType = true.into();
```

### Records and struct conversion

A `Record<V>` is an ordered key-value map (field name → value). It's how vantage
represents a single row of data regardless of the backend. The question is: how do
your structs become Records and vice versa?

There are two paths depending on your `value_type`.

#### Path A: serde_json::Value (the easy path)

If your type system uses `serde_json::Value` as the value type (like SQLite does),
you get struct conversion for free. Vantage has blanket implementations of `IntoRecord`
and `TryFromRecord` for any type that implements serde's `Serialize`/`Deserialize`:

```rust
#[derive(Serialize, Deserialize)]
struct Product {
    name: String,
    price: i64,
    is_deleted: bool,
}

let product = Product { name: "Cupcake".into(), price: 120, is_deleted: false };

// Struct → Record<serde_json::Value> — automatic via serde
let record: Record<serde_json::Value> = product.into_record();

// Record<serde_json::Value> → Struct — automatic via serde
let restored: Product = Product::from_record(record).unwrap();
```

This works because serde already knows how to turn structs into JSON objects and back.
No extra code needed on your part.

#### Path B: Custom value type (the #[entity] path)

If your type system uses something other than `serde_json::Value` — like SurrealDB's
`ciborium::Value` — then serde's blanket impls don't apply. You need the `#[entity]`
proc macro to generate the conversion code:

```rust
#[entity(SurrealType)]
#[derive(Debug, Clone)]
struct Product {
    name: String,
    price: i64,
    is_deleted: bool,
}
```

The `#[entity(SurrealType)]` macro looks at each field, and generates:

- `IntoRecord<AnySurrealType>` — calls `AnySurrealType::new(self.name)` for each field
- `TryFromRecord<AnySurrealType>` — calls `record["name"].try_get::<String>()` for each field

This is where the type markers from Step 1 become critical. When you read a record
back from the database, each value is an `AnySurrealType` with a variant tag. The
`try_get::<String>()` call checks that the variant is `Text` before extracting. If
someone stored an integer in a field that should be a string, you get an error instead
of garbage.

You can even target multiple type systems at once:

```rust
#[entity(SurrealType, CsvType)]
struct Product {
    name: String,
    price: i64,
}
```

This generates conversions for both `Record<AnySurrealType>` and `Record<AnyCsvType>`,
so the same struct works across different backends.

#### Testing Record conversions

Test `Record<AnySqliteType>` in both modes — typed (write path) and untyped (read path).

**Typed records** simulate what you'd build when inserting data. Values have variant
tags, and `try_get` enforces them:

```rust
#[test]
fn test_typed_record() {
    let mut record: Record<AnySqliteType> = Record::new();
    record.insert("name".into(), AnySqliteType::new("Cupcake".to_string()));
    record.insert("price".into(), AnySqliteType::new(120i64));

    assert_eq!(record["name"].try_get::<String>(), Some("Cupcake".to_string()));
    assert_eq!(record["name"].try_get::<i64>(), None);  // Text ≠ Integer → blocked
    assert_eq!(record["price"].try_get::<i64>(), Some(120));
    assert_eq!(record["price"].try_get::<String>(), None);  // Integer ≠ Text → blocked
}
```

**Untyped records** simulate what comes back from the database. Values have
`type_variant: None`, so `try_get` is permissive — it just attempts the conversion:

```rust
#[test]
fn test_untyped_record() {
    let mut record: Record<AnySqliteType> = Record::new();
    record.insert("name".into(), AnySqliteType::untyped(json!("Cupcake")));
    record.insert("price".into(), AnySqliteType::untyped(json!(120)));

    assert_eq!(record["name"].try_get::<String>(), Some("Cupcake".to_string()));
    assert_eq!(record["name"].try_get::<i64>(), None);  // fails because "Cupcake" isn't a number
    assert_eq!(record["price"].try_get::<i64>(), Some(120));
    assert_eq!(record["price"].try_get::<f64>(), Some(120.0));  // permissive — json 120 can be f64
}
```

The key difference: a typed `AnySqliteType::new(42i64)` blocks `try_get::<f64>()`
because Integer ≠ Real. An untyped `AnySqliteType::untyped(json!(42))` allows it
because there's no variant to check — it just asks "can JSON number 42 be read as f64?"

Also test `Option<T>` fields, null handling, and missing fields.

### TryFrom<AnyType> for common types

You also need `TryFrom<AnyType>` implementations for scalar types and Records.
These are used later by `AssociatedExpression::get()` in Step 2, but they belong
here because they're part of the type system:

```rust
// Scalars — extract single value from single-row results
impl TryFrom<AnySqliteType> for i64 { ... }
impl TryFrom<AnySqliteType> for String { ... }
// etc.

// Records — extract first row from result array
impl TryFrom<AnySqliteType> for Record<AnySqliteType> { ... }
impl TryFrom<AnySqliteType> for Record<serde_json::Value> { ... }
```

For scalars, if the result is a single-row, single-column array like `[{"COUNT(*)": 3}]`,
extract the value automatically. For Records, extract the first row and wrap each
field as an untyped `AnyType`.

### Step 1 conclusion

At this point you should have:

1. **Type impls** in `src/<backend>/types/` — the `vantage_type_system!` macro call,
   trait implementations for each Rust type, `From` conversions on `AnyType`,
   variant detection in `TypeVariants::from_*()`, and `TryFrom<AnyType>` for
   scalars and Records.

2. **Tests** in `tests/<backend>/1_types_round_trip.rs` covering:
   - In-memory `AnyType` round-trips for each supported type
   - Type mismatch rejections (wrong variant → `None`)
   - Struct ↔ Record conversions (including Option fields and error cases)
   - Values read from the actual database converting correctly

### How type markers flow through the system

The `AnyType` wrapper has an `Option<Variant>` field — `Some(Integer)` means "I know
this is an integer", `None` means "I don't know, just try whatever conversion you need."

**Writing (you → database):** Values created with `AnySqliteType::new(42i64)` get
`type_variant: Some(Integer)`. The bind layer uses the variant to pick the right sqlx
bind call — Integer binds as `i64`, Text as `&str`, Real as `f64`. No guessing.

**Reading (database → you):** Values coming back from the database are created with
`AnySqliteType::untyped(json_value)` which sets `type_variant: None`. This means
`try_get::<i64>()` won't be blocked by a variant mismatch — it just attempts the
conversion. The type checking happens later when you deserialize into a struct.

```rust
// Writing — typed, variant enforced
let val = AnySqliteType::new(true);            // type_variant: Some(Bool)
val.try_get::<bool>();   // Some(true) — variant matches
val.try_get::<i64>();    // None — Bool ≠ Integer, blocked by type boundary
val.try_get::<String>(); // None — Bool ≠ Text, blocked

// Reading — untyped, permissive
let val = AnySqliteType::untyped(json!(1));    // type_variant: None
val.try_get::<i64>();    // Some(1) — no variant check, json 1 parses as i64
val.try_get::<bool>();   // Some(true) — no variant check, json 1 parses as bool (≠0)
val.try_get::<f64>();    // Some(1.0) — no variant check, json 1 parses as f64
val.try_get::<String>(); // None — json 1 can't parse as String
```

Both directions use `Record<AnySqliteType>`, but the values behave differently:

```
Writing:  Struct → Record<AnySqliteType> (typed) → bind_sqlite_value() → sqlx
Reading:  sqlx → Record<AnySqliteType> (untyped) → try_get / serde → Struct
```


## Step 2: Make expressions work

With the type system in place, you can now use `Expression<AnySqliteType>` to build
and execute queries. This step has two deliverables: a convenience macro and the
`ExprDataSource` trait implementation.

### The vendor macro

Define a macro that produces `Expression<YourAnyType>`. SurrealDB has `surreal_expr!`,
we create `sqlite_expr!`:

```rust
let expr = sqlite_expr!("SELECT * FROM product WHERE price > {}", 100i64);
```

Under the hood, `100i64` gets wrapped as `AnySqliteType::new(100i64)` with variant
`Integer`. When this expression hits the database, the bind layer knows to call
`query.bind(100i64)` — not `query.bind("100")` or `query.bind(100.0)`.

The macro handles three kinds of parameters:
- `42i64` → scalar with type marker
- `(sub_expr)` → nested expression (composed into the template)
- `{deferred}` → lazy evaluation (resolved at execution time)

### ExprDataSource

Implement `DataSource` (marker) and `ExprDataSource<AnySqliteType>` on your DB struct.
The `execute` method takes an expression, flattens nested sub-expressions, converts
`{}` placeholders to your driver's syntax (`?N` for SQLite, `$N` for Postgres), binds
parameters using type markers, and returns results.

Results come back as `AnySqliteType` with `type_variant: None` — the database doesn't
preserve our markers, so results are permissive (see Step 1). For SQLite that's
especially natural since it doesn't distinguish boolean from integer on the wire.

If the persistence layer you're implementing *does* preserve type information in
responses (like SurrealDB with CBOR tags), set the correct `type_variant` when
constructing result values in your `execute()` implementation. That way `try_get`
enforces type boundaries on both sides of the round-trip.

### Validating with INSERT expressions

The best way to test this is INSERT + SELECT round-trips. A single insert exercises
all the pieces — macro, parameter binding, type markers, and result parsing:

```rust
let insert = sqlite_expr!(
    "INSERT INTO product (id, name, price, is_deleted) VALUES ({}, {}, {}, {})",
    "cupcake", "Flux Cupcake", 120i64, false
);
db.execute(&insert).await?;

let select = sqlite_expr!("SELECT * FROM product WHERE id = {}", "cupcake");
let result = db.execute(&select).await?;
```

Nested expressions let you build multi-row inserts from composable parts:

```rust
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
```

The `ExpressionFlattener` collapses all nesting into one flat template with positional
parameters — each one still carrying its type marker for correct binding.

### Deferring: cross-database value resolution

Sometimes a query on one database needs a value from another database. That's what
`defer()` is for — it wraps a query as a closure that executes later, when the outer
query runs.

This is not a subquery. The deferred query runs first, produces a concrete value, and
that value gets bound as a regular parameter in the outer query.

```rust
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
```

Your `execute()` implementation needs to resolve deferred parameters before flattening.
Walk the parameter list, call `.call().await` on any `Deferred`, and leave `Scalar`
and `Nested` untouched.

The resolved value comes back as an untyped `AnySqliteType` (no variant marker), so
it gets bound via JSON-inference. For SQLite this is fine — the loose type system
handles it. For stricter databases, you may want `defer()` to preserve type
information from the source query's result.

### Reading query results

So far we've been calling `db.execute(&expr).await` which returns `AnySqliteType`.
For a SELECT query, that value wraps a JSON array of row objects. To work with
individual rows, you convert into Records:

```rust
let result = db.execute(&sqlite_expr!("SELECT * FROM product")).await?;

// Result is AnySqliteType wrapping [{"id":"a","name":"Cheap","price":50}, ...]
// Convert to records manually:
let rows: Vec<JsonValue> = match result.into_value() {
    JsonValue::Array(arr) => arr,
    _ => panic!("expected rows"),
};
let record: Record<JsonValue> = rows[0].clone().into();
```

That works but it's verbose. The `TryFrom<AnyType>` impls from Step 1 make this
cleaner through `AssociatedExpression`. When you call `db.associate::<R>(expr)`,
you get an expression that knows its return type — `.get()` executes and converts
in one step:

```rust
// Scalar — extracts single value from single-row result
let count = db.associate::<i64>(sqlite_expr!("SELECT COUNT(*) FROM product"));
assert_eq!(count.get().await?, 3);

// Record — extracts first row
let record: Record<JsonValue> = db
    .associate(sqlite_expr!("SELECT * FROM product WHERE id = {}", "c"))
    .get().await?;
```

From a Record, you can deserialize into a struct. For the `#[entity]` path:

```rust
#[entity(SqliteType)]
struct Product { id: String, name: String, price: i64 }

let record: Record<AnySqliteType> = db
    .associate(sqlite_expr!("SELECT * FROM product WHERE id = {}", "c"))
    .get().await?;
let product = Product::from_record(record)?;
```

Or for the serde path with `Record<JsonValue>`:

```rust
#[derive(Deserialize)]
struct Product { id: String, name: String, price: i64 }

let record: Record<JsonValue> = db
    .associate(sqlite_expr!("SELECT * FROM product WHERE id = {}", "c"))
    .get().await?;
let product: Product = Product::from_record(record)?;
```

Testing the failure modes (missing fields, NULL into required field, wrong types)
can help spot issues in your implementation.

### Step 2 conclusion

At this point you should have:

1. **A vendor macro** (`sqlite_expr!`, `surreal_expr!`, etc.) that produces
   `Expression<AnyType>` with typed parameters.

2. **Trait impls** in `src/<backend>/impls/` — `DataSource` (marker) and
   `ExprDataSource<AnyType>` with `execute()` and `defer()`.

3. **Tests** in `tests/<backend>/2_*.rs` covering:
   - INSERT with typed parameters, read back and verify
   - Multi-row INSERT using nested expressions and `from_vec`
   - Type marker verification (bool binds as bool, not as string "true")
   - Cross-database deferred value resolution
   - AssociatedExpression with scalar, Record, and entity results

## Step 3: Statement builders and SelectableDataSource

In practice, nobody writes raw expressions for every query. This step adds the
`Selectable` trait implementation for your SELECT builder and wires it up through
`SelectableDataSource` so the rest of vantage can create and execute queries
through a standard interface.

### Implement Selectable for your SELECT builder

The `Selectable<T>` trait is the standard interface for building SELECT queries
across all vantage backends. Your SELECT struct needs to implement it. The trait
has two kinds of methods:

**Mutating methods** you must implement — `set_source`, `add_field`,
`add_where_condition`, `add_order_by`, `add_group_by`, `set_limit`, `set_distinct`,
the `clear_*` methods, the `has_*` methods, `as_count`, and `as_sum`.

**Builder methods** you get for free — `with_source`, `with_field`, `with_condition`,
`with_order`, `with_expression`, `with_limit`. These are default implementations
that call the mutating methods and return `self`.

This means your builder code is just the struct definition and `new()`:

```rust
pub struct SqliteSelect {
    pub fields: Vec<Expr>,
    pub from: Vec<Expr>,
    pub where_conditions: Vec<Expr>,
    pub order_by: Vec<(Expr, bool)>,
    pub group_by: Vec<Expr>,
    pub distinct: bool,
    pub limit: Option<i64>,
    pub skip: Option<i64>,
}

impl SqliteSelect {
    pub fn new() -> Self { /* initialize empty */ }
}
```

The `Selectable` impl goes in its own file (e.g., `statements/select/impls/selectable.rs`)
and the builder methods come from the trait:

```rust
let select = SqliteSelect::new()
    .with_source("product")
    .with_field("name")
    .with_field("price")
    .with_condition(sqlite_expr!("\"is_deleted\" = {}", false))
    .with_order(sqlite_expr!("\"price\""), false)
    .with_limit(Some(10), None);
```

The `as_count()` and `as_sum()` methods should clone the current query, replace
the fields with `COUNT(*)` or `SUM(column)`, drop the ORDER BY (unnecessary for
aggregates), and render:

```rust
let count_expr = select.as_count();  // SELECT COUNT(*) FROM product WHERE ...
let sum_expr = select.as_sum(sqlite_expr!("\"price\""));  // SELECT SUM("price") FROM ...
```

### Implement SelectableDataSource

This trait connects the SELECT builder to execution. It tells vantage "this
database can create SELECT queries and run them":

```rust
impl SelectableDataSource<AnySqliteType> for SqliteDB {
    type Select = SqliteSelect;

    fn select(&self) -> Self::Select {
        SqliteSelect::new()
    }

    async fn execute_select(&self, select: &Self::Select) -> Result<Vec<AnySqliteType>> {
        // delegate to ExprDataSource::execute()
    }
}
```

### Live tests

Up to now, most tests used in-memory databases created in `setup()`. For Step 3,
start running tests against a real pre-populated database. This catches issues
that in-memory tests miss — schema mismatches, type affinity surprises, data
edge cases.

Set up a test database with known data (we use a bakery schema translated from
SurrealDB's test fixture), and write tests that query it through the `Selectable`
interface:

```rust
let db = SqliteDB::connect("sqlite:../target/bakery.sqlite?mode=ro").await?;

let select = SqliteSelect::new()
    .with_source("product")
    .with_condition(sqlite_expr!("\"price\" > {}", 200i64))
    .with_order(sqlite_expr!("\"price\""), false);

let record: Record<AnySqliteType> = db.associate(select.expr()).get().await?;
let product = Product::from_record(record)?;
assert_eq!(product.name, "Enchantment Under the Sea Pie");
```

### Other statements

`Selectable` only covers SELECT. INSERT, UPDATE, and DELETE don't need a trait
at this stage — they just need to implement `Expressive<AnySqliteType>` so they
can be passed to `ExprDataSource::execute()`. The statement builders from earlier
steps still work, they just need their expression type migrated from `JsonValue`
to `AnySqliteType` to flow directly into `execute()`.

### Step 3 conclusion

At this point you should have:

1. **`Selectable<AnyType>` impl** for your SELECT builder — all standard methods
   implemented, builder pattern provided by trait defaults.

2. **`SelectableDataSource<AnyType>` impl** for your DB struct — `select()` and
   `execute_select()`.

3. **Tests** in `tests/<backend>/3_*.rs` covering:
   - SQL rendering via `preview()` — fields, conditions, ordering, limits, distinct, group by
   - `as_count()` and `as_sum()` render correctly
   - Live execution against a test database — SELECT, COUNT, SUM, ORDER+LIMIT
   - Entity deserialization from live query results
