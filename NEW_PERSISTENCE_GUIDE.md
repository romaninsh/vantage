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

### Step 1 conclusion

At this point you should have:

1. **Type impls** in `src/<backend>/types/` — the `vantage_type_system!` macro call,
   trait implementations for each Rust type, `From` conversions on `AnyType`, and
   variant detection in `TypeVariants::from_*()`.

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


## Step 2: Implement ExprDataSource

The expression system is how vantage builds and executes queries without knowing
which database you're talking to. An `Expression<T>` is a template string with `{}`
placeholders and a list of typed parameters. The type parameter `T` is your `AnyType`
— this is what makes the type markers from Step 1 flow through to binding.

### Create a vendor-specific expression macro

Each backend defines a convenience macro that produces `Expression<AnyType>`. Vantage
already ships `surreal_expr!` for SurrealDB. For SQLite, we define `sqlite_expr!`:

```rust
#[macro_export]
macro_rules! sqlite_expr {
    ($template:expr) => {
        Expression::<AnySqliteType>::new($template, vec![])
    };
    ($template:expr, $($param:tt),*) => {
        Expression::<AnySqliteType>::new(
            $template,
            vec![ $(vantage_expressions::expr_param!($param)),* ]
        )
    };
}
```

The `expr_param!` helper handles the three kinds of parameters:
- bare value like `42i64` → `ExpressiveEnum::Scalar(42i64.into())`
- parenthesized expression like `(sub_expr)` → `ExpressiveEnum::Nested(sub_expr.expr())`
- braced deferred like `{deferred_fn}` → `ExpressiveEnum::Deferred(...)`

Now building expressions is clean:

```rust
// Simple query — parameters carry type markers automatically
let expr = sqlite_expr!("SELECT * FROM product WHERE price > {}", 100i64);

// The 100i64 becomes AnySqliteType with Integer variant,
// which bind_sqlite_value() uses to call query.bind(100i64)
```

There's also `sql_expr!` which produces `Expression<serde_json::Value>` — the untyped
variant. That's used by statement builders (Step 3) where types are inferred from
context. For `ExprDataSource::execute()`, always use the typed variant.

### Expressions compose

Expressions nest naturally. Each sub-expression keeps its own parameters with their
type markers intact:

```rust
// Build rows as nested expressions, each with typed parameters
let row1 = sqlite_expr!("({}, {}, {})", "cupcake", 120i64, false);
let row2 = sqlite_expr!("({}, {}, {})", "tart", 220i64, false);

// Combine into a single INSERT — Expression::from_vec joins with ", "
let rows = Expression::from_vec(vec![row1, row2], ", ");
let insert = Expression::<AnySqliteType>::new(
    "INSERT INTO product (name, price, is_deleted) VALUES {}",
    vec![ExpressiveEnum::Nested(rows)],
);

db.execute(&insert).await?;
```

When executed, the `ExpressionFlattener` collapses all nesting into a flat template
with only scalar parameters — safe for positional binding.

### What you need to implement

Two traits on your DB struct:

```rust
impl DataSource for SqliteDB {}  // just a marker

impl ExprDataSource<AnySqliteType> for SqliteDB {
    async fn execute(&self, expr: &Expression<AnySqliteType>) -> Result<AnySqliteType> {
        // 1. Flatten nested expressions
        // 2. Convert {} placeholders to your driver's bind syntax (?N for SQLite)
        // 3. Bind the scalar parameters using type-aware bind_sqlite_value()
        // 4. Execute and convert rows back to AnySqliteType
    }

    fn defer(&self, expr: Expression<AnySqliteType>) -> DeferredFn<AnySqliteType> {
        // Return a closure that calls execute() later — used for lazy evaluation
    }
}
```

The key helper is `prepare_typed_query` — it flattens the expression and converts
`{}` placeholders to your driver's syntax:

```rust
fn prepare_typed_query(expr: &Expression<AnySqliteType>) -> (String, Vec<AnySqliteType>) {
    let flattener = ExpressionFlattener::new();
    let flattened = flattener.flatten(expr);
    // Walk the template, replace each {} with ?1, ?2, ...
    // Collect scalar AnySqliteType values in order
    (sql_string, params)
}
```

Each database driver has its own placeholder syntax. SQLite and MySQL use `?N`,
Postgres uses `$N`, SurrealDB uses `$_argN`. The flattener doesn't care — it just
gives you a flat list of scalars and a template.

Then the bind step uses the type markers from Step 1:

```rust
fn bind_sqlite_value(query, value: &AnySqliteType) -> ... {
    match value.type_variant() {
        Some(SqliteTypeVariants::Integer) => query.bind(value.as_i64()),
        Some(SqliteTypeVariants::Text) => query.bind(value.as_str()),
        Some(SqliteTypeVariants::Real) => query.bind(value.as_f64()),
        // ...
    }
}
```

### Reading results back

`ExprDataSource::execute()` returns an `AnySqliteType`. For queries that return rows,
this wraps a JSON array of objects. To get typed structs out:

```rust
let result = db.execute(&sqlite_expr!("SELECT * FROM product WHERE id = {}", "cupcake")).await?;

// Unwrap the array
let rows = match result.into_value() {
    serde_json::Value::Array(arr) => arr,
    _ => panic!("expected array"),
};

// Each row → Record → struct via serde
let record: Record<serde_json::Value> = rows[0].clone().into();
let product: Product = Product::from_record(record)?;
```

Remember the asymmetry from Step 1: writing uses `AnySqliteType` (type-aware binding),
reading returns `Record<serde_json::Value>` (struct validates types via serde).

### Step 2 conclusion

At this point you should have:

1. **A vendor macro** (`sqlite_expr!`, `surreal_expr!`, etc.) that produces
   `Expression<AnyType>` with typed parameters.

2. **Trait impls** in `src/<backend>/impls/` — `DataSource` (marker) and
   `ExprDataSource<AnyType>` with `execute()` and `defer()`.

3. **Tests** in `tests/<backend>/2_*.rs` covering:
   - SELECT with parameterized queries (each type)
   - INSERT with typed parameters, read back and verify
   - Nested expressions (compose sub-expressions, execute as one query)
   - Type marker verification (bool binds as bool, not as string "true")
   - Empty result sets
