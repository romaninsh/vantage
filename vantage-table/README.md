# Vantage Table

Type-safe table abstractions with ActiveEntity support for database-agnostic CRUD operations.

## Quick Start

```rust
use vantage_dataset::prelude::*;
use vantage_table::table::Table;

#[derive(Debug, Clone)]
struct User {
    name: String,
    email: String,
    active: bool,
}

// Create table with any compatible data source
let table = Table::new("users", data_source)
    .with_column_of::<String>("name")
    .with_column_of::<String>("email")
    .with_column_of::<bool>("active");

// Load and modify entities
let mut user = table.get_entity(&"user123".to_string()).await?
    .unwrap_or_else(|| table.new_entity("user123".to_string(), User {
        name: "New User".to_string(),
        email: "user@example.com".to_string(),
        active: false,
    }));

// Direct field modification
user.active = true;
user.email = "updated@example.com".to_string();

// Automatic persistence
user.save().await?;
```

## Features

- **ActiveEntity Pattern**: Load entities, modify fields directly, and save changes automatically
- **Database Agnostic**: Works with CSV, PostgreSQL, MySQL, SQLite, SurrealDB, MongoDB, and custom
  data sources
- **Type Safety**: Full compile-time validation of entity structure and field access
- **Flexible Queries**: Integration with vantage-expressions for complex query building
- **Relationships**: Same-persistence traversal via `with_one`/`with_many`, cross-persistence via
  `with_foreign`

## Core Operations

### Data Source

A `Table` is parameterized by its data source — the persistence that stores and retrieves records.
You pass it when constructing the table, and it determines what condition syntax you use, how
queries execute, and what types flow through:

```rust
// PostgreSQL — SQL expressions, connection pool
let pg_db = PostgresDB::connect("postgres://localhost/mydb").await?;
let users = Table::new("users", pg_db)
    .with_id_column("id")
    .with_column_of::<String>("name");

// MongoDB — BSON documents, collection-based
let mongo_db = MongoDB::connect("mongodb://localhost:27017", "mydb").await?;
let users = Table::new("users", mongo_db)
    .with_id_column("_id")
    .with_column_of::<String>("name");

// CSV — file-based, read-only
let csv = Csv::new("data/");
let users = Table::new("users", csv)
    .with_column_of::<String>("name");
```

The same entity struct works with any persistence. Model crates typically provide a constructor per
persistence:

```rust
impl User {
    pub fn postgres_table(db: PostgresDB) -> Table<PostgresDB, User> { ... }
    pub fn mongo_table(db: MongoDB) -> Table<MongoDB, User> { ... }
    pub fn csv_table(csv: Csv) -> Table<Csv, User> { ... }
}
```

### Columns

Define table structure with typed columns for precise querying:

```rust
let users_table = Table::new("users", data_source)
    .with_column_of::<String>("name")
    .with_column_of::<i64>("age")
    .with_column_of::<bool>("active");

// Add conditions to filter records
let active_users = users_table
    .with_condition(users_table["active"].eq(true));

// Work with filtered dataset
for mut user in active_users.list_entities().await? {
    user.last_login = Utc::now();
    user.save().await?;
}
```

### Conditions

Conditions can be added in two ways.

**Using columns and operators** — persistence-agnostic, works the same everywhere. The `Operation`
trait provides `.eq()`, `.gt()`, `.lt()`, `.gte()`, `.lte()`, `.ne()`, `.in_()` on any column:

```rust
// Works on PostgreSQL, SQLite, MongoDB, SurrealDB — any persistence
products.add_condition(products["is_deleted"].eq(false));
products.add_condition(products["price"].gt(100));
```

**Using persistence-specific syntax** — for queries that need native features beyond what the
generic operators express:

```rust
// PostgreSQL — vendor-specific SQL function
let mut products = Table::new("product", pg_db);
products.add_condition(postgres_expr!(
    "LENGTH({}) > {} AND {} = ANY({})",
    (ident("name")),
    10i64,
    "cupcake",
    (ident("tags"))
));
```

```rust
// MongoDB — native BSON operators
let mut products = Table::new("product", mongo_db);
products.add_condition(doc! {
    "price": { "$gte": 100, "$lte": 500 },
    "tags": { "$elemMatch": { "$eq": "seasonal" } }
});
```

Either way, the effect is the same: the table now addresses a subset of your records. Every
subsequent operation honours the conditions — `list()`, `get_count()`, `get_sum()`, `delete_all()`,
relationship traversal, and any other operation on the table. A conditioned table isn't a query
result — it's a narrowed view of your data set.

### Data Modeling

Entity-based modeling is a way to abstract persistence behind a unified interface. A single model
file defines how an entity is stored — columns, relationships, business rules — for each persistence
it needs to support. The rest of your application works with the model and doesn't know or care
where the data lives:

```rust
// models/user.rs

#[entity(PostgresType, SqliteType)]
#[derive(Debug, Clone, Default)]
pub struct User {
    pub name: String,
    pub email: String,
    pub tier: String,
}

impl User {
    /// Primary storage — PostgreSQL
    pub fn from_db(db: PostgresDB) -> Table<PostgresDB, User> {
        Table::new("user", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("tier")
            .with_many("orders", "user_id", Order::from_db)
            .with_one("active_subscription", "id", |db| {
                let mut t = Subscription::from_db(db);
                t.add_condition(t["is_active"].eq(true));
                t
            })
    }

    /// Local cache — SQLite (same entity, different persistence)
    pub fn from_cache(db: SqliteDB) -> Table<SqliteDB, User> {
        Table::new("user_cache", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("tier")
            .with_column_of::<chrono::NaiveDateTime>("cache_expires")
    }

    /// In-memory mock — for tests
    #[cfg(test)]
    pub fn from_mock(data: Vec<User>) -> Table<MockTableSource, User> {
        let ds = MockTableSource::from_data("user", data);
        Table::new("user", ds)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("tier")
    }
}
```

`Table<PostgresDB, User>` retains full type information — you have access to PostgreSQL-specific
expressions, typed columns, and relationship traversal. A trait on the typed table can expose domain
methods that hide persistence details completely. The caller doesn't know table names, column
layouts, or SQL syntax — and may not even be aware whether data comes from PostgreSQL or has been
migrated to a REST API:

When you need to pass a table into generic code that doesn't care about the persistence type (CLI
rendering, admin panels, API handlers), wrap it with `AnyTable::from_table()` to erase the types —
see the [AnyTable](#anytable) section below.

```rust
pub trait UserTable {
    fn current_user(&self, id: &str) -> Self;
    fn ref_orders(&self) -> Table<PostgresDB, Order>;
    fn ref_active_subscription(&self) -> Table<PostgresDB, Subscription>;
    fn discount_expr(&self) -> AssociatedExpression<'_, PostgresDB, AnyPostgresType, f64>;
}

impl UserTable for Table<PostgresDB, User> {
    fn current_user(&self, id: &str) -> Self {
        let mut t = self.clone();
        t.add_condition(t["id"].eq(id));
        t
    }

    fn ref_orders(&self) -> Table<PostgresDB, Order> {
        self.get_ref_as::<Order>("orders").unwrap()
    }

    fn ref_active_subscription(&self) -> Table<PostgresDB, Subscription> {
        self.get_ref_as::<Subscription>("active_subscription").unwrap()
    }

    /// Discount based on order history — returns a composable expression.
    /// Uses CASE to tier the discount by order count.
    fn discount_expr(&self) -> AssociatedExpression<'_, PostgresDB, AnyPostgresType, f64> {
        let orders = self.ref_orders();
        let order_count = Fx::new("count", [ident("id").expr()]);

        let discount = Case::new()
            .when(order_count.clone().gt(50), 20.0f64)
            .when(order_count.clone().gt(10), 15.0f64)
            .when(order_count.clone().gt(0), 10.0f64)
            .otherwise(0.0f64);

        let mut select = orders.select();
        select.clear_fields();
        select.add_expression(discount, Some("discount".into()));

        self.data_source().associate::<f64>(select.expr())
    }
}
```

Rest of your business logic doesn't build raw SQL or reference column names. If the storage changes,
only the model file changes. Rest of the code is unchanged or has minimal changes:

```rust
// Today — discount computed via PostgreSQL subquery
let discount = users.discount_expr().get().await?;

// Tomorrow — persistence switched to RestAPI, same caller code
let discount = users.discount_expr().get().await?;  // unchanged
```

### Relationships

Vantage has unique relationship traversal. References are defined through expressions, traversing
them is almost zero-cost:

```rust
let active_subscription = current_user.ref_active_subscription();
let orders = current_user.ref_orders();
```

You can decide when to execute queries and how:

```rust
if active_subscription.get_some_value().await?.is_some() {
    let order_count = orders.get_count().await?;
}
```

Vantage also supports foreign references via `with_foreign()` — relationships that cross
persistence boundaries (e.g. PostgreSQL users → billing API subscriptions). The closure
receives the source table and returns an `AnyTable` with deferred conditions. See the
[vantage-expressions README](vantage-expressions/README.md) for details on `DeferredFn` and
cross-persistence query building.

### AnyTable

`AnyTable` erases the persistence and entity types, exposing a uniform `serde_json::Value`-based
interface. This is useful for generic code — CLI tools, API handlers, admin panels — that doesn't
need to know which database is behind it:

```rust
// Wrap any typed table
let any_table = AnyTable::from_table(Product::postgres_table(db));

// Same interface regardless of persistence
let records = any_table.list_values().await?;
let count = any_table.get_count().await?;
any_table.insert_value(&id, &record).await?;
any_table.delete(&id).await?;
```

A CLI tool that works with multiple persistence sources at runtime:

```rust
let table: AnyTable = match source {
    "postgres" => AnyTable::from_table(Product::postgres_table(pg_db)),
    "mongo"    => AnyTable::from_table(Product::mongo_table(mongo_db)),
    "csv"      => AnyTable::from_table(Product::csv_table(csv)),
    _ => panic!("unknown source"),
};

// All commands work identically — list, count, insert, delete
let records = table.list_values().await?;
render_records(&records);
```

Values flow through as `serde_json::Value` — booleans render as `true`/`false`, numbers stay
numeric, nulls render cleanly. Your persistence's type system (defined in Step 1 of the persistence
guide) ensures values arrive with the right JSON type rather than everything being a string.

Most application code works directly with `Table<T, E>` instead — this gives access to typed
entities, persistence-specific conditions, and relationship traversal. `AnyTable` is for the layer
above, where persistence choice is a runtime decision.

### Loading Records

There are multiple ways to load records depending on your needs:

- **`get_entity(id)`** — Load ActiveEntity by ID (returns `None` if not found)
- **`get_value(id)`** — Load raw record data by ID
- **`list()`** — Load all raw entities as `IndexMap<Id, Entity>`
- **`list_entities()`** — Load all entities as `Vec<ActiveEntity>`
- **`list_values()`** — Load all raw record data

```rust
// Load raw entities (no save functionality)
let users: IndexMap<String, User> = table.list().await?;
for (id, user) in users {
    println!("User {}: {} ({})", id, user.name, user.email);
}

// Load raw record data for inspection
if let Ok(record) = table.get_value(&"user123".to_string()).await {
    println!("Raw data: {:?}", record);
    println!("Name field: {:?}", record["name"]);
}
```

### Get-or-Create Pattern

```rust
let mut user = table.get_entity(&"user123".to_string()).await?
    .unwrap_or_else(|| table.new_entity("user123".to_string(), User::default()));
```

### Table Metadata

Various ways to interact with table metadata and statistics:

```rust
// Get record count
let count: i64 = table.get_count().await?;

// Get count as AssociatedExpression for use in other queries
let count_expr = table.get_table_expr_count();
let result = count_expr.get().await?;

// Aggregated values
let max_age = table.get_table_expr_max(&table["age"]);
let max_value = max_age.get().await?;

// Select query builder
let select = table.get_select_query();
let results = select
    .with_field("name")
    .with_condition(expr!("active = true"))
    .with_limit(10)
    .execute().await?;
```

## Diverse Data Persistence Support

Vantage Table works with a wide range of data sources through modular adapter crates:

### Available Adapters

- **vantage-sql**: PostgreSQL, MySQL, SQLite via vendor-aware SQL generation
- **vantage-mongodb**: MongoDB with native BSON conditions (no SQL)
- **vantage-surrealdb**: SurrealDB with native SurrealQL
- **vantage-csv**: CSV file persistence with type-safe column parsing
- **vantage-api-client**: REST API persistence with pagination
- **[`MockTableSource`]**: In-memory testing and development mock

### Create Your Own Persistence Adapter

A persistence adapter plugs into the Vantage framework by implementing traits at two levels. Once
implemented, `Table<T, E>` automatically bridges these into the full `ReadableValueSet`,
`WritableValueSet`, `ReadableDataSet<E>`, and `WritableDataSet<E>` trait families — you do not need
to implement those yourself.

#### Required Traits

| Trait                     | Crate         | Purpose                                                                                                                                                                                                          |
| ------------------------- | ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`TableSource`**         | vantage-table | The core trait. Defines associated types (`Column`, `AnyType`, `Value`, `Id`, `Condition`) and all CRUD + aggregation methods. Also provides `related_in_condition` for same-persistence relationship traversal. |
| **`ColumnLike<AnyType>`** | vantage-table | Column metadata for your persistence's column type (`name`, `alias`, `flags`, `get_type`). You can use the built-in `Column<T>` or create a custom column type.                                                  |

#### Optional Traits

| Trait                  | Crate               | Purpose                                                                                                                                                                            |
| ---------------------- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`ExprDataSource`**   | vantage-expressions | Execute expressions and create deferred closures. Required by SQL persistence for subquery-based relationship traversal. Not needed by document-oriented persistence like MongoDB. |
| **`TableExprSource`**  | vantage-table       | Expression-returning aggregations (`get_table_expr_count`, `get_table_expr_max`) for use in subqueries or deferred execution.                                                      |
| **`TableQuerySource`** | vantage-table       | SELECT query builder support for persistence that can compose structured queries.                                                                                                  |

#### What You Get for Free

Once `TableSource` is implemented, `Table<T, E>` provides:

- **`ReadableValueSet`** / **`WritableValueSet`** — raw record CRUD (delegates to your TableSource)
- **`ReadableDataSet<E>`** / **`WritableDataSet<E>`** — typed entity CRUD with automatic
  Record-Entity conversion
- **`ActiveEntitySet<E>`** — change-tracked entity wrappers with `.save()`
- **`TableLike`** — dyn-safe interface with conditions, pagination, ordering, and search
- **`AnyTable`** — type-erased wrapper for heterogeneous table collections
- **Relationship traversal** — `with_one()`, `with_many()`, `with_foreign()`, `get_ref_as()`,
  `get_ref()`

#### Supporting Crates

- **vantage-types**: Define your persistence type system with `vantage_type_system!` macro
- **vantage-expressions**: Build query primitives supporting nesting and cross-database operations
- **vantage-core**: Unified error handling across the ecosystem

#### Simple Alternative: Direct DataSet Implementation

For persistence that doesn't need table/column abstractions (e.g. in-memory stores), you can
implement the vantage-dataset traits directly:

- `ReadableValueSet` + `WritableValueSet` + `InsertableValueSet` (raw record layer)
- `ReadableDataSet<E>` + `WritableDataSet<E>` + `InsertableDataSet<E>` (typed entity layer)

See `ImTable` in vantage-dataset for a reference implementation of this approach.

## Integration

Part of the Vantage framework:

- **vantage-types**: Type system, entity definitions, and `TerminalRender` trait
- **vantage-expressions**: Query building and database abstraction
- **vantage-dataset**: CRUD operation traits
- **vantage-core**: Error handling and utilities
- **vantage-csv**: CSV file data source
- **vantage-sql**: SQL persistence (PostgreSQL, MySQL, SQLite)
- **vantage-mongodb**: MongoDB persistence
- **vantage-surrealdb**: SurrealDB persistence
- **vantage-cli-util**: Terminal table rendering utilities
