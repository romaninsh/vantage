# Step 4: Table Abstraction and Entity CRUD

The same entities get used hundreds of times across a codebase — constructing a query from scratch
every single time is tedious and error-prone. Vantage offers `Table<>` as an abstraction over your
entity definitions: it knows the table name, the columns, their types, and the ID field, so it can
build queries for you.

To use your persistence backend as a table source, you need to implement the `TableSource` trait.
Most of the heavy-lifting is done by the `vantage-table` crate — your job is to implement
`TableSource` trait methods.

### Implement TableSource with placeholder methods

Start by adding the required dependencies:

```toml
# in your backend's Cargo.toml
vantage-table = { path = "../vantage-table" }
async-trait = "0.1"
```

Create a new test file (e.g. `tests/<backend>/4_table_def.rs`) that defines a table and populates
its columns. The columns rely on the type system you built in Step 1.

The `TableSource` implementation also declares several associated types:

- **`Column`** — the `Column` type supplied by `vantage-table` is good enough for most backends.
- **`AnyType`** and **`Value`** — your type-erased value type from Step 1 (e.g. `AnySqliteType`).
- **`Id`** — use `String` for SQL databases, or a custom type if your IDs have special structure
  (e.g. SurrealDB's `Thing` which encodes `table:id`). Whatever you pick must be covered by your
  type system.

```rust
use async_trait::async_trait;
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::traits::table_source::TableSource;

#[async_trait]
impl TableSource for SqliteDB {
    type Column<Type> = Column<Type> where Type: ColumnType;
    type AnyType = AnySqliteType;
    type Value = AnySqliteType;
    type Id = String;
    // ...
}
```

Implement the following methods first — they're all straightforward delegations:

- **Column management** — `create_column`, `to_any_column`, `convert_any_column`:

```rust
    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type> {
        Column::new(name)
    }

    fn to_any_column<Type: ColumnType>(
        &self,
        column: Self::Column<Type>,
    ) -> Self::Column<Self::AnyType> {
        Column::from_column(column)
    }

    fn convert_any_column<Type: ColumnType>(
        &self,
        any_column: Self::Column<Self::AnyType>,
    ) -> Option<Self::Column<Type>> {
        Some(Column::from_column(any_column))
    }
```

- **Expression factory** — `expr()`:

```rust
    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<ExpressiveEnum<Self::Value>>,
    ) -> Expression<Self::Value> {
        Expression::new(template, parameters)
    }
```

Every other method — should start as `todo!()`. You'll implement them incrementally in the following
sections, driven by tests.

### Define entity tables

With `TableSource` in place, define your entity structs and table constructors. The pattern is the
same across all backends — `#[entity(YourType)]` for the struct, plus a builder method that returns
`Table<YourDB, Entity>`:

```rust
use vantage_sql::sqlite::{SqliteType, SqliteDB, AnySqliteType};
use vantage_table::table::Table;
use vantage_types::entity;

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Product {
    name: String,
    calories: i64,
    price: i64,
    bakery_id: String,
    is_deleted: bool,
    inventory_stock: i64,
}

impl Product {
    fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<String>("bakery_id")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<i64>("inventory_stock")
    }
}
```

Note that the entity struct does **not** include the `id` field — that's handled separately by
`with_id_column()`, which registers the column and sets the table's ID field. The remaining columns
are added with `with_column_of::<Type>()`, which creates typed columns via your
`TableSource::create_column` implementation.

### Verify with a query generation test

Your first test should build a table, then call `table.select()`. Just like the Step 3 tests, you
can use `preview()` to check the rendered SQL, and later execute it against a real database:

```rust
#[tokio::test]
async fn test_product_select() {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    let table = Product::sqlite_table(db);
    let select = table.select();
    assert_eq!(
        select.preview(),
        "SELECT \"id\", \"name\", \"calories\", \"price\", \
         \"bakery_id\", \"is_deleted\", \"inventory_stock\" FROM \"product\""
    );
}
```

This works because `table.select()` (provided by `vantage-table`) calls your
`SelectableDataSource::select()` to get a fresh SELECT builder, then applies the table name via
`set_source()` and adds each registered column via `add_field()`. None of the `todo!()` methods are
hit — only the column and expression infrastructure you already implemented.

### Implement the read methods

`Table<T, E>` implements two traits from `vantage-dataset` that provide read access:

- **`ReadableValueSet`** — returns raw `Record<Value>` (untyped storage values):
  - `list_values()` → all records as `IndexMap<Id, Record<Value>>`
  - `get_value(id)` → `Option<Record<Value>>` — `None` if no record matches the id
  - `get_some_value()` → one arbitrary record (or `None` if empty)

- **`ReadableDataSet<E>`** — returns deserialized entities (calls `E::try_from_record()` for you):
  - `list()` → all entities as `IndexMap<Id, E>`
  - `get(id)` → `Option<E>` — `None` if no entity matches the id
  - `get_some()` → one arbitrary entity

Both traits delegate to three `TableSource` methods: `list_table_values`, `get_table_value`, and
`get_table_some_value`. The pattern is the same for all three:

1. Get the id field name from `table.id_field()` (falls back to `"id"`)
2. Build a SELECT using `table.select()` (which already applies columns, conditions, ordering)
3. Execute via `self.execute(&select.expr())`
4. Parse the result — split each row into an ID and a `Record`

For `get_table_value`, add a WHERE condition on the id field and return `Ok(None)` when the
lookup misses — errors are reserved for actual connection or parse failures.
For `get_table_some_value`, set `LIMIT 1` and return the first row (or `None` if empty).

Write tests for both `ReadableValueSet` and `ReadableDataSet` in separate files — import the traits
from `vantage_dataset` and call `list_values()`, `get_value()`, `get_some_value()`, `list()`,
`get()`, `get_some()` against your pre-populated test database. Keep these tests condition-free —
conditions get their own test file next.

### Error handling

All `TableSource` methods return `vantage_core::Result<T>` (an alias for `Result<T, VantageError>`).
Use the `error!` macro from `vantage_core` to create errors with structured context:

```rust
use vantage_core::error;

// Simple error message
return Err(error!("expected array result"));

// With key = value context (NOT format args — the macro uses a different syntax)
return Err(error!("row missing id field", field = id_field_name));

// For database-specific errors, convert them with map_err
let rows = query.fetch_all(self.pool()).await
    .map_err(|e| error!("SQLite query failed", details = e.to_string()))?;
```

The macro automatically captures file, line, and column. The `key = value` pairs are stored as
structured context, not interpolated into the message string.

To wrap external errors with additional context, use the `Context` trait:

```rust
use vantage_core::Context;

// Wraps the original error as the "source" of a new VantageError
let data = std::fs::read("config.json")
    .context(error!("failed to load config"))?;
```

This chains errors — the original `io::Error` is preserved as the source, so `Display` renders both
messages and the source chain is available via `std::error::Error::source()`.

### Operation trait — condition building

Each backend provides an operation trait (e.g. `SqliteOperation`) with `.eq()`, `.ne()`, `.gt()`,
`.gte()`, `.lt()`, `.lte()`, and `.in_()` methods for building conditions. It has a **blanket
implementation** for all `Expressive<T>` types, so your columns get these methods automatically — no
explicit impl needed.

All methods accept `impl Expressive<YourAnyType>`, so you can pass native Rust values (`false`,
`42`, `"hello"`), other columns (`table["other_field"]`), or full expressions. This requires your
scalar types to implement `Expressive<YourAnyType>` — the same impls you added in Step 1 for the
vendor macro.

### Testing conditions

`Table` carries conditions set via `add_condition()`, and `table.select()` applies them
automatically as WHERE clauses. Test a few patterns:

- **Custom expression** — pass columns as expression arguments via `table["field"]`:

```rust
let mut table = Product::sqlite_table(db);
table.add_condition(sqlite_expr!("{} > {}", (table["price"]), 130));
```

- **Multiple conditions** — combined with AND, including field-to-field comparison:

```rust
let mut table = Product::sqlite_table(db);
table.add_condition(sqlite_expr!("{} > {}", (table["price"]), 130));
table.add_condition(sqlite_expr!("{} > {}", (table["price"]), (table["calories"])));
```

- **SqliteOperation::eq()** — the idiomatic way:

```rust
use vantage_sql::sqlite::operation::SqliteOperation;

let mut table = Product::sqlite_table(db);
table.add_condition(table["is_deleted"].eq(false));
```

### Implement aggregates

Implement `get_table_count`, `get_table_sum`, `get_table_max`, and `get_table_min` in your
`TableSource`. These build aggregate queries from `table.select()` and extract the scalar result.
Once implemented, `Table` exposes shorter `get_count`, `get_sum`, `get_max`, `get_min` methods
directly:

```rust
let table = Product::sqlite_table(db);
assert_eq!(table.get_count().await.unwrap(), 5);
assert_eq!(table.get_max(&table["price"]).await.unwrap().try_get::<i64>().unwrap(), 299);
```

### Implement write operations

`Table` also implements `WritableDataSet` (insert, replace, patch, delete) and `InsertableDataSet`
(insert with auto-generated ID). These delegate to six `TableSource` methods:

- **`insert_table_value`** — INSERT with a known ID. Build an `SqliteInsert` with the id field and
  record fields, execute, then read back via `get_table_value`.
- **`replace_table_value`** — full replacement. For SQLite, use `INSERT OR REPLACE INTO`.
- **`patch_table_value`** — partial update. Build an `SqliteUpdate` with only the provided fields
  and a WHERE condition on the id field.
- **`delete_table_value`** — DELETE with a WHERE condition on the id field.
- **`delete_table_all_values`** — DELETE without conditions.
- **`insert_table_return_id_value`** — INSERT without a known ID (auto-increment). Use
  `RETURNING "id"` to get the generated ID back from the database.

Test both `WritableValueSet` (raw records, no entity) and `WritableDataSet` (typed entities) using
in-memory SQLite:

```rust
// WritableValueSet — no entity needed
let rec = record(&[("name", "Gamma".into()), ("price", 30i64.into())]);
table.insert_value(&"c".to_string(), &rec).await.unwrap();

// WritableDataSet — typed entities
let item = Item { name: "Gamma".into(), price: 30 };
table.insert(&"c".to_string(), &item).await.unwrap();

// InsertableDataSet — auto-generated ID
let id = table.insert_return_id(&item).await.unwrap();
let fetched = table.get(id).await.unwrap();
```

