# Vantage Expressions

A composable database-agnostic expression framework for Rust that enables building
SQL-injection-safe queries using templates and parameters. Provides the foundation for creating
advanced query builders that can implement any query language and database flavor. Implementation is
well suited for large distributed code-bases and enterprise-level migration / refactoring projects.

## Example Use

```rust,ignore
use vantage_expressions::expr;

let where_expr = expr!("age > {} AND status = {}", 21, "active");
let query_expr = expr!("SELECT * FROM users WHERE {}", (where_expr));
```

The [`expr!`] macro keeps your parameters separate from the query template, preventing SQL injection
while maintaining readability.

For a complete example with testing using mockbuilder,
[see expression module documentation](crate::expression::expression).

## Features

- **[`expr!`] macro**: SQL-injection safe query building with great support for Rust native types
- **Dynamic query construction**: Compose queries conditionally using [`Expression::from_vec()`]
- **Deferred execution**: Embed async API calls or Mutexes directly in queries with [`DeferredFn`]
- **Cross-database type mapping**: Handle type conversion when query crosses persistence boundaries.
- **Custom SQL constructs**: Implement [`Expressive`] trait for UNION, CTE, or vendor-specific
  syntax
- **Standardized SELECT builders**: [`Selectable`] trait works across SQL, SurrealDB, MongoDB

A design goal for Vantage Expressions is to assist with Enterprise refactoring - providing powerful
mechanisms to perform persistence refactoring without breaking model API and affectign existing
code.

Expressions are just one part of Vantage framework. I recommend also looking into:

- vantage-surrealdb - Implements SurrealDB client SDK by integrating it into Vantage, providing many
  `Expressive` constructs, `Selectable` implementation, Precise type system.
- vantage-dataset - Although not directly related to Expressions - DataSet crate provides similar
  abstraction for abstractign CRUD operatons on remote data sources.
- vantage-table - Provides abstract implementation with schema-compliant data-sources. Although
  `Table` does not rely on expressions directly - it makes sense to implement those through
  `Selectable` query building.
- vantage-core - Implements `VantageError`, `Return` and some other useful things shared across
  Vantage crates.

## Rust Type Support

Expressions can carry any universal type of your choosing. The [`expr!`] macro defaults to
`serde_json::Value` for maximum compatibility, but you can use [`Expression<T>`] with any type that
suits your database. Use CBOR values for binary protocols, SurrealDB's native types for SurrealQL,
or design custom enum-style types optimized for your specific database's type system.

```rust,ignore
use vantage_expressions::expr_any;
use surrealdb::sql::Value as SurrealValue;
use std::time::Duration;

// Using SurrealDB native types with Duration
let surreal_query = expr_any!(
    SurrealValue,
    "SELECT * FROM session WHERE created_at > time::now() - {}",
    Duration::from_secs(3600)
);
```

This approach is different from a "type binding" employed in crates like SQLx, where all parameters
must be provided at-once and without abstraction.

## Dynamic Query Building

Expressions can be built dynamically at runtime, allowing for flexible query construction based on
your application logic. use [`Expression::from_vec`] to join multiple expressions with a separator,
preserving order of all parameters stored within.

```rust,ignore
let mut conditions = Vec::new();

// Conditionally build WHERE conditions
if let Some(age) = min_age {
    conditions.push(expr!("age >= {}", age));
}
if let Some(status) = status {
    conditions.push(expr!("status = {}", status));
}
if active_only {
    conditions.push(expr!("last_login > NOW() - INTERVAL 30 DAY"));
}

// Combine conditions using from_vec
let where_clause = Expression::from_vec(conditions, " AND ");
let final_query = expr!("SELECT * FROM users WHERE {}", (where_clause));
```

If you design a query builder for a custom query language, you want maximum freedom, even allowing
your API to accept user-supplied expression.

## Type Mapping

Expressions can be converted between compatible types using the mapping functionality. This is
useful when you need to convert `Expression<String>` to `Expression<Value>` or between other
compatible types:

```rust
use vantage_expressions::{Expression, ExpressiveEnum, expression::mapping::ExpressionMap};
use serde_json::Value;

// Create expression with String parameters
let string_expr: Expression<String> = Expression::new(
    "SELECT * FROM users WHERE name = {}",
    vec![ExpressiveEnum::Scalar("John".to_string())],
);

// Convert to Expression<Value> using the map() method
let value_expr: Expression<Value> = string_expr.map();
```

Type mapping handles all expression components automatically:

- **Scalar values** are converted using the `Into` trait
- **Nested expressions** are converted recursively
- **Deferred values** are wrapped in conversion closures that execute at runtime

This enables seamless interoperability between different expression types while maintaining type
safety.

### Cross-Database Queries with Type Mapping

Type mapping becomes particularly powerful when combined with deferred queries across databases with
incompatible value types:

```rust,ignore
use vantage_expressions::{expr, protocol::datasource::QuerySource, expression::mapping::ExpressionMap};

// Database 1 uses String values, Database 2 uses JSON Values
let db1 = StringDatabase::new("connection1");
let db2 = JsonDatabase::new("connection2");

// Create query for db1 and defer its execution
let string_query = expr!("SELECT user_ids FROM active_users WHERE department = {}", "engineering");
let deferred_query = db1.defer(string_query);

// Map the deferred String query to JSON Value and execute on db2
let result = db2.execute(&deferred_query.map()).await;
```

The deferred query from `db1` is automatically converted from `Expression<String>` to
`Expression<Value>` when mapped, enabling cross-database operations even when the databases use
incompatible value types.

## Type Mapping

If your database engine uses a custom type system (e.g. SurrealType) but under the hood it would use
CBOR it is sufficient for you to implement `Into<cborium::Value>`. Now any expression defined for
your custom type can be mapped into cborium::Value automatically.

Here is example of mapping [`Expression<String>`] into [`Expression<Value>`]:

```rust,ignore
// Create expression with String parameters
let string_expr: Expression<String> = Expression::new(
    "SELECT * FROM users WHERE name = {}",
    vec![ExpressiveEnum::Scalar("John".to_string())],
);

// Convert to Expression<Value> using the map() method
let value_expr: Expression<Value> = string_expr.map();
```

Ability to map expression values is important when system must operate across different databases
and each database could be implementing their own type system.

### Immediate vs deferred execution

`vantage-expression` does not require you to implement database SDK in a certain way, however, by
implementing a trait [`QuerySource`] your SDK would have 2 foundational methods:

- `async db.execute(expr) -> result` - Execute an [`Expression<V>`] now and return `Result<V>`.
- `db.defer(expr) -> DeferredFn` - Wrap query execution into a closure which can be executed with
  `fn.call()`

```rust,ignore
// Create a query expression
let query = expr!("SELECT COUNT(*) FROM users WHERE age > {}", 21);

// Immediate execution - execute now and get result
let count: serde_json::Value = db.execute(&query).await?;
println!("User count: {}", count);

// Deferred execution - create a closure for later execution
let deferred_query = db.defer(query.clone());

// Execute the deferred query when needed
let count_later = deferred_query.call().await?;
match count_later {
    ExpressiveEnum::Scalar(value) => println!("Deferred count: {}", value),
    _ => println!("Unexpected result type"),
}
```

## Other kinds of `DeferredFn`

A sharp-eyeed reader would notice that `count_later` actually contains
[`ExpressiveEnum::Scalar(value)`]. As it turns out - resolving deferred query can also return nested
expressions, once return results are known. I'll explore the powerful implications of non-scalar
return types later.

There are also other ways to obtain [`DeferredFn`], for instance you can create it from a mutex:

```rust,ignore
// Shared state that can change over time
let counter = Arc::new(Mutex::new(10));

// Create deferred function from mutex - reads current value when executed
let deferred_count = DeferredFn::from_mutex(counter.clone());
let query = expr!("SELECT * FROM items LIMIT {}", { deferred_count });

// Change the value after query construction
*counter.lock().unwrap() = 25;

// When executed, the query uses the current value (25), not the original (10)
let result = db.execute(&query).await?;
```

## Associated Expressions

While deferred execution provides powerful async capabilities, sometimes you need a middle ground
between immediate execution and full deferral.

Associated expressions combine 3 things - Expression, DataSource reference and Expected type. For
example - imagine a method, `get_authenticated_users_email`. Should it query and return email or
return Expression? Thin can be both now:

```rust,ignore
use vantage_expressions::{expr, ExprDataSource, AssociatedExpression};

// Get authenticated user's email with type safety
fn get_authenticated_users_email(ds: &impl ExprDataSource<serde_json::Value>)
    -> AssociatedExpression<'_, _, serde_json::Value, Email> {
    let query = expr!(
        "SELECT email FROM users WHERE id = (SELECT user_id FROM sessions WHERE token = current_session())"
    );
    ds.associate::<Email>(query)
}
```

This can now be used to get Email directly or inside expressions. Also - We using a custom type for
Email, so we don't loose on type-safety (check vantage_types) for more info on type mapping support.

```rust,ignore
// Direct execution with type safety
let email: Email = get_authenticated_users_email(&db).get().await?;
println!("User: {}@{}", email.name, email.domain);
```

Using as part of other query:

```rust,ignore
// Use in other queries via composition
let balance_query = expr!(
    "SELECT balance FROM accounts WHERE email = {}",
    (get_authenticated_users_email(&db))
);
let balance = db.execute(&balance_query).await?;
```

Unlike DeferredFn - you will need a proper data source for AssociatedExpression.

## Cross-database query-building

The main purpose of deferred queries is to enable cross-database query building. Assume we start
with this query:

```sql
SELECT * FROM orders WHERE user_id IN (SELECT id FROM user WHERE status = 'active');
```

Converting this query into Expression is a sync operations. However if during your database refactor
`user` table migrates to an external API, this would break significant portion of your code as it
would require async API fetch.

Vantage uses deferred queries to solve this problem:

```rust,ignore
// API call that fetches user IDs asynchronously
async fn get_user_ids() -> vantage_core::Result<serde_json::Value> {
    // Simulate API call - fetch from external service
    Ok(serde_json::json!([1, 2, 3, 4, 5]))
}

// Build query synchronously - no async needed here!
let query = expr!("SELECT * FROM orders WHERE user_id = ANY({})", { DeferredFn::from_fn(get_user_ids) });

// Execute the query - API call happens automatically during execution
let orders = db.execute(&query).await?;
```

The query building remains synchronous even though `get_user_ids()` is an async API call. The API is
only invoked when the query is executed, maintaining clean separation between query construction and
execution phases.

The deferred SurrealDB query executes automatically when the PostgreSQL query needs the user IDs,
enabling seamless cross-database operations. Result is passed into db.execute() as a bind.

## Extensibility

Create custom SQL constructs by implementing the [`Expressive`] trait:

The `execute()` method handles all deferred operations and resolves them into final values. This
design allows the use of shared state like `Arc<Mutex<T>>` inside callbacks, enabling dynamic query
parameters that can change between query construction and execution.

```rust,ignore
/// A UNION SQL construct
#[derive(Clone)]
pub struct Union<T> {
    left: Expression<T>,
    right: Expression<T>,
}

impl<T: Clone> Expressive<T> for Union<T> {
    fn expr(&self) -> Expression<T> {
        Expression::new(
            "{} UNION {}",
            vec![
                ExpressiveEnum::Nested(self.left.clone()),
                ExpressiveEnum::Nested(self.right.clone()),
            ],
        )
    }
}

// Usage example with nested queries and stored procedure
let users_query = expr!("CALL get_active_users_by_dept({})", "engineering");
let admins_query = expr!("SELECT name FROM admins WHERE role = {}", "super");

let union = Union::new(users_query, admins_query);
let final_query = expr!("SELECT DISTINCT name FROM ({})", (union.expr()));
```

Vantage-expressions provides a solid foundation for implementing query builders for any dialect or
database language. Query builders can implement builders like `Select` or `Insert`, use composable
types like `Table`, `Aggregation`, `Sum`, and even implement operations like comparison methods
`eq()` or `ne()`.

## Selectable Trait

A most sophisticated construct usually is a `SELECT` builder. There can be a separate `INSERT` or
combined builder - that's up to a DB vendor, but a `SELECT` builder usually is quite common.

The [`Selectable`] trait provides a standardized interface for building SELECT-style queries across
different database backends. It defines common operations like filtering, sorting, field selection,
and pagination that are universal to most query languages.

```rust,ignore
use vantage_expressions::{expr, protocol::selectable::Selectable};
use vantage_surrealdb::select::SurrealSelect;

// Create a new select query builder
let mut select = SurrealSelect::new();

// Build query using Selectable trait methods
select.set_source(expr!("users"), None);
select.add_field("name".to_string());
select.add_field("email".to_string());
select.add_expression(expr!("age * 2"), Some("double_age".to_string()));
select.add_where_condition(expr!("age > {}", 18));
select.add_where_condition(expr!("active = {}", true));
select.add_order_by(expr!("name"), true);
select.add_group_by(expr!("department"));
select.set_distinct(true);
select.set_limit(Some(10), Some(5));

// Convert to expression and execute
let query_expr: Expression = select.into();
let result = db.execute(&query_expr).await?;
```

The [`Selectable`] trait also provides fluent builder-style methods for chaining operations:

```rust,ignore
let query = SurrealSelect::new()
    .with_source(expr!("products"))
    .with_field("name")
    .with_field("price")
    .with_condition(expr!("price > {}", 100))
    .with_order(expr!("price"), false)
    .with_limit(Some(5), None);

let results = db.execute(&query.expr()).await?;
```

Database-specific implementations like `SurrealSelect` implement the [`Selectable`] trait while
providing their own syntax and features. This allows the same query building patterns to work across
SQL, SurrealDB, MongoDB, and other backends while maintaining database-specific optimizations.

## Use of [`Expression`] across Vantage framework

As you have probably noticed - [`Selectable`] trait makes use of nested expressions quite
deliberatly. Vantage framework treats expressions as a first class citizen and therefore we want to
expose interface which is powerful and extensive.

This extensibility makes Vantage a cohesive framework, suitable for the use in the Enterprise
setting.
