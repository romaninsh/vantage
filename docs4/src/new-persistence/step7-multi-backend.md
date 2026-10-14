# Step 7: Multi-Backend Applications

At this point your backend works — you can define tables, query data, and traverse relationships.
But a real application typically has a _model crate_ that defines entities once and offers table
constructors for each backend. That's `bakery_model3` in the Vantage repo. The final piece is
type erasure, which lets you treat tables from different backends uniformly.

```admonish info title="AnyTable is gone — use Vista"
Earlier versions erased the backend with `AnyTable`. It was removed in 0.5.2; type erasure now
lives one layer up in [`Vista`](./step8-vista-integration.md), reached through the driver's vista
factory. The mechanism below is the supported replacement.
```

### Vista: the type-erased handle

`db.vista_factory().from_table(table)` erases the backend and entity types behind a uniform,
schema-bearing [`Vista`](./step8-vista-integration.md) carrying `Record<ciborium::Value>`. This is
what makes it possible to write generic UI, CLI, or API code that doesn't care which database is
behind it:

```rust
# use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
# use vantage_table::table::Table;
# use vantage_types::entity;
# #[entity(SqliteType)]
# #[derive(Debug, Clone)]
# struct Product {
#     name: String,
#     price: i64,
# }
# impl Product {
#     fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Product> {
#         Table::new("product", db)
#             .with_id_column("id")
#             .with_column_of::<String>("name")
#             .with_column_of::<i64>("price")
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let db = SqliteDB::connect("sqlite::memory:").await?;
// Wrap any typed Table<Driver, Entity> as a backend-agnostic Vista:
let products = db.vista_factory().from_table(Product::sqlite_table(db.clone()))?;
// (or surreal/csv/mongo — the call site is identical)
# Ok(())
# }
```

Erasure works because each driver's vista factory bridges its native `AnyType` to the CBOR value
the Vista exposes — the `vantage_type_system!` macro and your Step 1 conversions already provide
both directions. See [Step 8](./step8-vista-integration.md) for the factory and `TableShell` you
implement to enable this.

### Building a multi-source CLI

The CLI example in `bakery_model3/examples/cli.rs` shows the pattern. A `build_table` function
matches on the user's chosen source, calls the right entity constructor, and wraps it through the
vista factory. Once you have a `Vista`, all commands are backend-agnostic — listing, counting,
reading, inserting, and deleting all work identically regardless of which database is behind it.

Because the values flow through as a typed CBOR record, the CLI renderer can inspect types at
runtime — booleans like `is_deleted` display as `true`/`false` with color highlighting, numbers stay
numeric, and nulls render cleanly. Your type system work in Step 1 ensures these values arrive with
the right type rather than everything being a string.

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
`vista_factory().from_table()` bridges the gap between your backend's native types and a uniform
record-based interface.
