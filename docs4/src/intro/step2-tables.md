# Tables and Typed Data Access

A table is a structure representing a collection of records in a database:

```rust
let products = Product::table(db);

for (id, product) in products.list().await? {
    println!("{} — {} cents", product.name, product.price);
}
```

Think of `Table<SqliteDB, Product>` as a `Vec<Product>` — except the records aren't in memory. They
live somewhere else (a database, a file, an API), and you don't know how many there are or what they
contain until you list them.

If you've used an ORM before, you might expect Vantage to work with individual records — load one,
change it, save it back. Vantage works differently: you always operate on a **set** of records. A
set might contain one record, no records, or millions — you don't pull them into memory to find out.

Operations like counting, filtering, and updating happen on the database side — Vantage builds the
right query and sends it over. Even traversal operations and sub-queries are done by building
queries and executing remotely — if the database supports it, of course.

When you `.clone()` a table, you don't clone the data — you clone the _definition_. From there you
can narrow it down by adding conditions, turning it into a subset. Some examples of data sets you
might work with:

- all user records except those with a soft-delete flag
- orders placed today
- paid customers
- orders of paid customers
- products that sold at least 5 items today

Each of these is a set — defined by conditions, not a list of IDs. Another typical operation is
expressing one set as a condition of another then perform an action:

- notify customers who have an unpaid invoice older than 30 days
- archive orders whose products have been discontinued
- apply a discount to customers who spent more than $500 this month
- list warehouses that stock at least one out-of-stock product
- flag reviews written by users who have been banned

Put these together and even complex operations are easy to express in code:

```rust
// "notify customers who have an unpaid invoice older than 30 days"
let mut overdue = Invoice::table(db);
overdue.add_condition(overdue["status"].eq("unpaid"));
overdue.add_condition(overdue["issued_at"].lt(days_ago(30)));

overdue.ref_customer().send_reminder().await;
```

```rust
// "apply a discount to customers who spent more than $500 this month"
let mut customers = Customer::table(db);
customers.add_expression("spent_this_month", |c| {
    c.subquery_orders().only_this_month().field_sum("total")
});
customers.add_expression("discount", |c| {
    primitives::ternary(c["spent_this_month"].gt(500), 10, 0)
});
```

Don't worry about the exact syntax — we'll get to all of it. The point is that sets compose
naturally: define one, use it to narrow another, then act on the result.

## Defining a Table

The syntax for defining a table looks similar to query building from chapter 1, but there are
important differences:

|                  | Query                                   | Table                                            |
| ---------------- | --------------------------------------- | ------------------------------------------------ |
| **Lifetime**     | Built, executed, dropped                | Sticks around, spawns many queries               |
| **Operations**   | One SQL statement (SELECT, INSERT, ...) | Higher-level CRUD: list, get, add, patch, delete |
| **Columns**      | String field names via `.with_field()`  | Typed `Column<T>` definitions                    |
| **Database**     | Not bound — just a struct               | Holds a database reference (`Arc`)               |
| **Idempotency**  | INSERT fails on duplicate key            | `replace()` and `delete()` are idempotent        |
| **Data sources** | Only databases with a query language    | Any data source: SQL, CSV, APIs, queues, etc.    |

A [`Table`](vantage_table::Table) is typically defined in its own file alongside the entity.
Create `src/product.rs`:

```rust
// src/product.rs
use vantage_sql::prelude::*;
use vantage_types::prelude::*;

#[entity(SqliteType)]
#[derive(Clone, Default)]
pub struct Product {
    pub name: String,
    pub price: i64,
}

impl Product {
    pub fn table(db: SqliteDB) -> Table<SqliteDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("price")
    }
}
```

```admonish tip title="Hiding the db argument"
In most cases you already know which database a table lives in — passing `db` every time
is noise. A common pattern is to wrap the connection in a global accessor:

~~~rust
impl Product {
    fn table() -> Table<SqliteDB, Product> {
        Table::new("product", get_sqlite_db())
            // ...columns...
    }
}
~~~

We'll keep passing `db` explicitly in this tutorial for clarity, but real applications
typically use this pattern.
```

This defines the `product` table once. You can now generate queries from it:

```rust
let table = Product::table(db);

// Full SELECT with all columns and conditions applied
let select = table.select();
println!("{}", select.preview());
// SELECT "id", "name", "price", "is_deleted" FROM "product"

// Count query (does not execute — just builds the expression)
let count_query = table.get_count_query();
// SELECT COUNT(*) FROM "product"

// Sum query for a specific column
let sum_query = table.get_sum_query(&table["price"]);
// SELECT SUM("price") FROM "product"
```

These return the same `SqliteSelect` and `Expression` types from chapter 1 — the table just
assembles them for you. But most of the time you won't need the raw query at all.

---

## CRUD operations

With the table defined, all four operations — create, read, update, delete — are one-liners.
Each comes from a trait in the prelude:

```rust
let table = Product::table(db);

// Read — list all, get one by ID
let all = table.list().await?;             // IndexMap<String, Product>
let pie = table.get("pie").await?;         // Product

// Create — insert with a known ID
let muffin = Product { name: "Muffin".into(), price: 175 };
table.insert(&"muffin".to_string(), &muffin).await?;

// Update — replace the entire record
let updated = Product { name: "Blueberry Muffin".into(), price: 195 };
table.replace(&"muffin".to_string(), &updated).await?;

// Delete
table.delete(&"muffin".to_string()).await?;
```

That's it. `list()` returns an `IndexMap<Id, Product>` — ordered and keyed by ID. `get()`
returns the entity or an error if the ID doesn't exist. There's also `get_some()` which
returns `Option` for when you're not sure there are any records, and `insert_return_id()`
for when you want the database to generate the ID.

```admonish tip title="Idempotent operations"
Try duplicating the `replace()` and `delete()` calls — the result is the same. Replacing
a record that already has the new values is a no-op. Deleting a record that's already gone
succeeds silently. This makes table operations safe to retry without worrying about
side effects.
```

---

## Type-erased tables

`Table<SqliteDB, Product>` is great when you know the types at compile time. But what if you
want to write a function that lists *any* table — products, orders, customers — without
knowing the entity type?

`AnyTable` wraps a concrete table and erases its type parameters. Values come back as
`Record<serde_json::Value>` instead of typed entities. You can iterate columns by name and
build a generic display:

```rust
async fn list_table(table: &AnyTable) -> VantageResult<()> {
    let columns = table.column_names();

    // Header
    print!("  {:<12}", "id");
    for col in &columns {
        print!("{:<16}", col);
    }
    println!();

    // Rows
    for (id, record) in table.list_values().await? {
        print!("  {:<12}", id);
        for col in &columns {
            let val = record.get(col).map(|v| format!("{}", v)).unwrap_or_default();
            print!("{:<16}", val);
        }
        println!();
    }
    Ok(())
}
```

Convert any typed table into an `AnyTable` with `from_table()`:

```rust
let table = Product::table(db);
let any = AnyTable::from_table(table);

list_table(&any).await?;
// id          name            price
// cupcake     "Cupcake"       120
// donut       "Doughnut"      135
// ...
```

Under the hood, `AnyTable` converts values to and from `serde_json::Value` on the fly.
Conditions, pagination, and all CRUD operations still work — you just lose compile-time
type safety on the entity fields. This is the trade-off: `AnyTable` lets you build generic
UI components, CLI tools, and admin panels that work with any table definition.

```admonish info title="Generics vs type erasure"
You can also write `list_table` using generics — that keeps type safety but limits you
to Rust callers:

~~~rust
async fn list_table<E: Entity + std::fmt::Debug>(
    table: &impl ReadableDataSet<E>,
) -> VantageResult<()> {
    for (id, entity) in table.list().await? {
        println!("  {}: {:?}", id, entity);
    }
    Ok(())
}
~~~

If you plan to expose tables outside of Rust (Python bindings, FFI, a web admin UI),
type erasure via `AnyTable` is the way to go.
```
