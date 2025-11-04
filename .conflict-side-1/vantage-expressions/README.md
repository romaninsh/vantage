# Vantage Expressions

A composable database-agnostic expression framework for Rust that enables building
SQL-injection-safe queries using templates and parameters.
Provides the foundation for creating
advanced query builders that can implement any query language and database
flavor.

## Example Use

```rust
use vantage_expressions::expr;

let where_expr = expr!("age > {} AND status = {}", 21, "active");
let query_expr = expr!("SELECT * FROM users WHERE {}", (where_expr));
```

The `expr!` macro keeps your parameters separate from the query template, preventing SQL injection while maintaining readability.

## Features

- **SQL Injection Safe**: Parameters are kept separate from templates using the `expr!` macro
- **Composable**: Expressions can contain other expressions for modular query building
- **Deferred Execution**: Closures can be embedded that resolve at query execution time
- **Cross-Database**: Same expression types work with SQL, MongoDB, SurrealDB and more
- **Type Safe**: Built-in support for Rust types through `ExpressiveEnum`
- **Async Ready**: Designed for async/await patterns with `QuerySource` trait
- **Extensible**: Implement `Selectable` trait to standardize query builders across backends

## Rust Type Support

Expressions can carry any universal type of your choosing. The `expr!` macro defaults to `serde_json::Value` for maximum compatibility, but you can use `Expression<T>` with any type that suits your database. Use CBOR values for binary protocols, SurrealDB's native types for SurrealQL, or design custom enum-style types optimized for your specific database's type system.

```rust
use vantage_expressions::expr_any;
use surrealdb::sql::Value as SurrealValue;
use std::time::Duration;

// Using SurrealDB native types with Duration
let surreal_query = expr_any!(SurrealValue, "SELECT * FROM session WHERE created_at > time::now() - {}", Duration::from_secs(3600));
```

SurrealDB has excellent support for duration types that are directly compatible with Rust's native `std::time::Duration`, enabling seamless time calculations in queries.

## Dynamic Query Building

Expressions can be built dynamically at runtime, allowing for flexible query construction based on conditions:

```rust
use vantage_expressions::{expr, Expression, expression::flatten::ExpressionFlattener, Flatten};
use serde_json::Value;

let mut conditions = Vec::<Expression<Value>>::new();

// Conditionally build WHERE conditions
let min_age = Some(25);
let status = Some("premium");
let active_only = true;

if let Some(age) = min_age {
    conditions.push(expr!("age >= {}", age));
}
if let Some(s) = status {
    conditions.push(expr!("status = {}", s));
}
if active_only {
    conditions.push(expr!("last_login > NOW() - INTERVAL 30 DAY"));
}

// Combine conditions using from_vec
let where_clause = Expression::from_vec(conditions, " AND ");
let final_query = expr!("SELECT * FROM users WHERE {}", (where_clause));

// Flatten to see the final structure
let flattener = ExpressionFlattener::new();
let flattened = flattener.flatten(&final_query);
println!("Template: {}", flattened.template);
println!("Parameters: {:?}", flattened.parameters);
```

This demonstrates dynamic query construction where conditions are built conditionally and combined using `from_vec()`. The flattening process reveals how nested expressions are organized into the final query structure.

## Executing and Deferring Expressions

By implementing `QuerySource`, your database can execute expressions immediately or defer them for later execution:

```rust
use vantage_expressions::{expr, protocol::datasource::QuerySource};

// Assume you've implemented QuerySource for your database
let db = MyDatabase::new("connection_string");
let query = expr!("SELECT COUNT(*) FROM users WHERE age > {}", 21);

// Execute immediately - returns result now
let count = db.execute(&query).await;

// Defer execution - returns closure that can be called later
let deferred_query = db.defer(query);
let count = deferred_query().await; // Execute when needed
```

Deferred expressions enable powerful patterns like cross-database queries where one database's result becomes a parameter in another database's query.

## Deferred Queries as Parameters

Deferred queries can be embedded directly as parameters in other expressions, creating powerful cross-database query patterns:

```rust
use vantage_expressions::expr;

// Get active user IDs from SurrealDB
let user_ids_query = expr!("SELECT id FROM user WHERE status = {}", "active");
let surreal_db = SurrealConnection::new();
let deferred_users = surreal_db.defer(user_ids_query);

// Use those IDs in a PostgreSQL query - [deferred] syntax for DeferredFn
let orders_query = expr!(
    "SELECT * FROM orders WHERE user_id = ANY({})",
    {deferred_users}
);

let postgres_db = PostgresConnection::new();
let orders = postgres_db.execute(&orders_query).await;
```

The `defer()` method returns a `DeferredFn` that can be used directly with the `[deferred]` syntax in expressions. The deferred SurrealDB query executes automatically when the PostgreSQL query needs the user IDs, enabling seamless cross-database operations without manual coordination.

## Async Control

In typical applications, query building is done synchronously. The `defer()` mechanism enables creating interdependencies between different databases without introducing async complexity during the building phase. You can construct complex multi-database queries using regular synchronous code, while all async operations are handled automatically when `execute()` is called.

The `execute()` method handles all deferred operations and resolves them into final values. This design allows the use of shared state like `Arc<Mutex<T>>` inside callbacks, enabling dynamic query parameters that can change between query construction and execution.

```rust
use std::sync::{Arc, Mutex};
use vantage_expressions::{expr, protocol::expressive::DeferredFn};

// Shared state that can change over time
let counter = Arc::new(Mutex::new(10));

// Create deferred function from mutex - will read current value when executed
let deferred_count = DeferredFn::from_mutex(counter.clone());

let query = expr!("SELECT * FROM items LIMIT {}", { deferred_count });

// Change the value after query construction
*counter.lock().unwrap() = 25;

// When executed, the query will use the current value (25), not the original (10)
let result = db.execute(&query).await;
```

This pattern enables building responsive applications where query parameters can be updated by other parts of the application while queries are being prepared for execution.

## Extensibility

You can create custom SQL constructs by implementing the `Expressive` trait. This allows building reusable, type-safe query components that integrate seamlessly with the expression system.

```rust
use vantage_expressions::{expr, Expression, protocol::expressive::{Expressive, ExpressiveEnum}};

/// A UNION SQL construct that combines two SELECT expressions
#[derive(Clone)]
pub struct Union<T> {
    left: Expression<T>,
    right: Expression<T>,
}

impl<T> Union<T> {
    pub fn new(left: Expression<T>, right: Expression<T>) -> Self {
        Self { left, right }
    }
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

// Usage example
let users_query = expr!("SELECT name FROM users WHERE active = {}", true);
let admins_query = expr!("SELECT name FROM admins WHERE role = {}", "super");

let union = Union::new(users_query, admins_query);
let final_query = expr!("SELECT DISTINCT name FROM ({})", (union.expr()));
```

Custom constructs like `Union` can be nested within other expressions, creating a composable query building system where complex SQL can be built from reusable components.

## Query Language Builders

Vantage-expressions provides a solid foundation for implementing query builders for any dialect or database language. Query builders can implement builders like `Select` or `Insert`, use composable types like `Table`, `Aggregation`, `Sum`, and even implement operations like comparison methods `eq()` or `ne()`.

Other crates in `Vantage` framework take full advancage of `vantage-expressions`, so expect most types to implement expressionable.
