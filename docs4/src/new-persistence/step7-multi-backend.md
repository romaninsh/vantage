# Step 6: Multi-Backend Applications

At this point your backend works — you can define tables, query data, and traverse relationships.
But a real application typically has a _model crate_ that defines entities once and offers table
constructors for each backend. That's `bakery_model3` in the Vantage repo. The final piece is
`AnyTable`, which lets you treat tables from different backends uniformly.

### AnyTable: the type-erased wrapper

`AnyTable` erases the backend and entity types behind a uniform `serde_json::Value`-based interface.
This is what makes it possible to write generic UI, CLI, or API code that doesn't care which
database is behind it.

There are two ways to create one:

```rust
// 1. If your backend already uses serde_json::Value (rare):
let any = AnyTable::new(my_table);

// 2. For backends with custom value types (the common case):
let any = AnyTable::from_table(Product::sqlite_table(db));
```

`from_table` works as long as your `AnyType` implements `Into<serde_json::Value>` and
`From<serde_json::Value>`. The `vantage_type_system!` macro generates the `Into` conversion
automatically, and after Step 1 your backend should have the `From` direction covered too.

### Building a multi-source CLI

The CLI example in `bakery_model3/examples/cli.rs` shows the pattern. A `build_table` function
matches on the user's chosen source, calls the right entity constructor, and wraps it with
`AnyTable::from_table()`. Once you have an `AnyTable`, all commands are backend-agnostic —
`list_values()`, `get_count()`, `get_some_value()`, `insert_value()`, and `delete()` all work
identically regardless of which database is behind it.

Because the values flow through as `serde_json::Value`, the CLI renderer can inspect types at
runtime — booleans like `is_deleted` display as `true`/`false` with color highlighting, numbers stay
numeric, and nulls render cleanly. Your type system work in Step 1 ensures these values arrive with
the right JSON type rather than everything being a string.

Try it out:

```bash
# List products from CSV
cargo run --example cli -- csv product list

# Same thing from SQLite
cargo run --example cli -- sqlite product list

# Count bakeries in SurrealDB
cargo run --example cli -- surreal bakery count

# Get a single product record
cargo run --example cli -- sqlite product get

# Insert a new record
cargo run --example cli -- surreal bakery add myid '{"name":"Test","profit_margin":10}'

# Delete a record
cargo run --example cli -- surreal bakery delete myid
```

That's the payoff of implementing a proper type system and `TableSource` — one line of
`AnyTable::from_table()` bridges the gap between your backend's native types and a uniform
JSON-based interface.
