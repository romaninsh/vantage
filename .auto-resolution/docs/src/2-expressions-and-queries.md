# Expressions and Queries

In Vantage, query is a dynamic representation of a SQL query. You already saw how `sql::Table` is
creating `sql::Query`, now it's time to learn how `sql::Query` works.

Query owes it's flexibility to `Expressions` or more specifically to a `Chunk` trait. Any type
implementing `Chunk` can be part of a Query. `Expression` is just a simplest implementation of
`Chunk` trait.

The main reason for using `Expression` is separation of SQL statements and its parameters. Treating
SQL as a string introduces a possibility for SQL injections:

```rust
let query = format!(
  "SELECT * FROM product WHERE name = \"{}\"",
  user_name
);
```

What if `user_name` contains `"` character? Expression is able to handle this:

```rust
let expression = Expression::new(
    "SELECT * FROM product WHERE name = {}",
    vec![json!("DeLorian Doughnut")]);

writeln!(expression.preview());
```

Expression holds `statement` and `parameters` separatelly. Here are some methods of `Expression`:

- `expr!()` - macro for creating new expression
- `new()` - constructor, used by the macro
- `Expression::empty()` - an empty expression
- `sql()` - return SQL statement (without parameters)
- `sql_final()` - returns SQL but will replace {} placeholders with $1, $2 as requested by an underlying SQL access library
- `params()` - return array of parameters
- `preview()` - will insert parameters into the statement and show it for preview purposes. Do not use for executing queries!
- `split()` - returns statement and parameters as a tuple
- `from_vec()` - combines multiple expressions into one using delimeter (like concat_ws)

## `expr!()` macro

Parameters in expressions can have of several types, like i64 or String or &str:

```rust
let expression = expr!("INSERT INTO user (name, age) VALUES ({}, {})", "John", 30);
```

This macro relies on `serde_json::json!` macro to convert parameters to `serde_json::Value`.

## ExpressionArc

`Expression` implements `Chunk` trait, however it can only hold static parameters. Sometimes
we want our expressions to be able to hold other `Chunk` types. This is where `ExpressionArc`
comes in:

`ExpressionArc` is similar to `Expression` but can contain Arc<Box<dyn Chunk>> as a parameter.
It even has a similar macro:

```rust
let expression = expr_arc!("INSERT INTO user (name, age) VALUES ({}, {})", "John", 30);
```

Now, we can also pass nested expressions to `expr_arc!`:

```rust
let expression = expr_arc!("INSERT INTO {} (name, age) VALUES ({}, {})", expr!("user"), expr!("John"), 30);
```

## Overview of ExpressionArc methods:

- `from_vec()` - combines multiple `Chunk`s into single expression using a delimiter (like concat_ws)
- `fx()` - handy way to create a function call: `fx!("UPPER", vec!["Hello"])`

Just like `Expression`, `ExpressionArc` implements `Chunk`, so can be nested. This feature is crucial
for building queries.

## Query type

A `Query` will consists of many parts, each being a `Chunk`. When query needs to be rendered, it will
render all of its parts recursively:

```rust
// example from Query::render_delete()
Ok(expr_arc!(
    format!("DELETE FROM {}{{}}", table),
        self.where_conditions.render_chunk()
    ).render_chunk())
```

Obviously you can extend this easily or even have your own version of `Query`. Generally it's not needed,
as `Query` is very flexible and diverse. It can also hold other queries recursively.

Locate `bakery_model/examples/3-query-builder.rs` for an example of a super-complex query syntax.

### Query Overview:

Let me establish a pattern first:

- `set_table()` - sets table for a query
- `set_source()` - similar to `set_table()` but QuerySource can be a table, another query or an expression
- `with_table()` - similar to `set_table()` but returns a modified Self

As with `Table` type earlier - `set_table()` adn `set_source()` are implemented as part of dyn-safe `SqlQuery` trait.
On other hand `with_table()` is only implemented by `Query` struct.

Here are some other methods:

- `new()` - returns a blank query
- `set_distinct()`, `with_distinct()` - includes `DISTINCT` keyword into a query
- `set_type()`, `with_type()` - sets query type (INSERT, UPDATE, DELETE, SELECT)
- `add_with()`, `with_with()` - adds a WITH subquery to a query
- `add_field()`, `with_field()` - adds a field to a query
- `with_field_arc()` - accepts `Arc<Box<dyn Chunk>>` as a field
- `with_column_field()` - simplified way to add a table columns to a query
- `without_fields()` - removes all fields from a query
- `with_where_condition()`, `with_having_condition()` - adds a condition to a query.
- `with_condition()`, `with_condition_arc()` - accepts `impl Chunk` and `Arc<Box<dyn Chunk>>` as a condition
- `with_join()` - adds a join to a query
- `with_group_by()`, `add_group_by()` - adds a group by to a query
- `with_order_by()`, `add_order_by()` - adds an order by to a query
- `with_set_field()`, `set_field_value()` - sets a field value for INSERT, UPDATE or REPLACE queries

`Query` relies on several sub-types: `QuerySource`, `QueryConditions`, `JoinQuery` etc.

## How Query uses Expressions ?

Lets look at some examples, which combine Expressions and Query:

```rust
let expr1 = expr!("name = {}", "John");
let expr2 = expr!("age > {}", 30);

let query = Query::new()
    .with_table("users", None)
    .with_column_field("id")
    .with_column_field("name")
    .with_condition(expr1)
    .with_condition(expr2);

writeln!(query.preview());

// renders into: SELECT id, name FROM users WHERE name = 'John' AND age > 30
```

Lets continue to build out and make `query` part of a bigger `query2`:

```rust
// query is from example above
let query2 = Query::new()
    .with_table("orders", None)
    .with_condition(expr_arc!("user_id in {}",
        query
            .clone()
            .without_fields()
            .with_column_field("id")
    ));

writeln!(query2.preview());

// renders into: SELECT * FROM orders WHERE user_id in (SELECT id FROM users WHERE name = 'John' AND age > 30)
```

## Summary

Vantage's query system leverages `Expressions` and the `Chunk` trait to build dynamic, safe, and
composable SQL queries. `Expressions` separate SQL statements from parameters, preventing injection
risks, while `ExpressionArc` extends this flexibility by supporting nested expressions.

`Queries` are constructed from multiple `Chunk`s, allowing complex operations like subqueries,
joins, and conditions to be rendered recursively.

`Query` methods like `with_table`, `with_field`, and `with_condition` make query building very
simple and customizable, while macros like `expr!` and `expr_arc!` simplify additional ways to
extend queries.

Next, I'll explain how `Expressions` and `Query` can be part of `Table` field expressions.
