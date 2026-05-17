# Defining Models for Diorama

This is for someone writing a model crate like `bakery_model3` — typed
entities, table constructors per backend, references between them. The good
news: nothing about your model has to know that Diorama exists. You write
your model the same way you would for plain Vista use. Diorama wraps it
without changing the contract.

The slightly less good news: a few of the choices you make in your model
crate affect how well Diorama-backed UIs and caches behave around it. This
document covers those.

## The pattern that already works

`bakery_model3` is the reference. The shape:

```rust
use vantage_sql::{Sqlite, SqliteType};
use vantage_table::Table;

#[entity(CsvType, SqliteType, SurrealType, MongoType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Product {
    pub name: String,
    pub calories: i64,
    pub price: i64,
    pub is_deleted: bool,
}

impl Product {
    pub fn sqlite_table(db: Sqlite) -> Table<Sqlite, Self> {
        Table::new("products", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_one("bakery", "bakery_id", crate::Bakery::sqlite_table)
    }

    pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Self> { /* ... */ }
}
```

A `Table<DB, Entity>` becomes a `Vista` via the backend's
`vista_factory().from_table(t)`. A `Vista` becomes a Dio via
`lens.make_dio(vista)`. None of those transitions require changes to your
model — write clean tables, and Diorama works.

## What metadata helps the most

Vista already carries everything Diorama needs. The same metadata that helps
Vista (column types, id column, reference targets) helps Diorama. The places
where models can be more or less helpful for Diorama-backed UI flows:

### Always declare the id column

`with_id_column("id")` is what makes records addressable across the system.
Diorama uses the id column for:

- The cache's primary key (the redb table's key column)
- `RecordScenery` lookups (`dio.record_scenery(id)`)
- Event bus addressing (`ChangeEvent::Updated { id }`)
- Coalescing pending writes against the same record

If your entity's id is named something other than `"id"` (an account
number, a SKU), declare it explicitly:

```rust
Table::new("products", db).with_id_column("sku")
```

Without an id column, Diorama assigns synthetic identifiers based on row
position. That's fine for some workloads (logs, append-only data) but breaks
anything that needs to refer to a specific record later. Don't rely on it
for entities you'll edit.

### Mark columns with semantic flags

`vantage-vista` has column flags that describe the role of a column in the
UI and in queries. The ones Diorama and its consumers care about:

- `TITLE` — the column to show when this entity is referenced from elsewhere.
  A grid showing orders displays "Customer: John Smith" by reading the
  customer's `TITLE` column.
- `SEARCHABLE` — quicksearch matches against this column when Diorama falls
  back to local search. Without `SEARCHABLE` flags, the local search filter
  doesn't know which columns to scan.
- `ORDERABLE` (planned) — columns the UI should expose as sort options.
  Without flags, sort UI guesses (often wrongly) from the type.

```rust
Table::new("products", db)
    .with_id_column("id")
    .with_column(Column::new("name").with_flag(flags::TITLE).with_flag(flags::SEARCHABLE))
    .with_column(Column::new("description").with_flag(flags::SEARCHABLE))
    .with_column_of::<i64>("price")
    .with_column_of::<i64>("calories")
```

You don't need every column flagged — only the ones whose role in the UI is
non-obvious. The closer your flags match user intent, the less manual UI
config the consumer writes.

### References enable navigation and aggregates

`with_one` and `with_many` aren't just for typed-table traversal in your
business code — they describe the graph Diorama and UI consumers can walk.

```rust
Table::new("orders", db)
    .with_id_column("id")
    .with_column_of::<i64>("total")
    .with_one("customer", "customer_id", Customer::sqlite_table)
    .with_one("bakery", "bakery_id", Bakery::sqlite_table)
```

In a Diorama'd UI, double-clicking an order row and traversing to `customer`
opens a `RecordScenery` against the customer Dio with the customer's id from
the row. A `ValueScenery` showing "this bakery has 47 unfilled orders" walks
the `with_many("orders", ...)` reference to compute the aggregate.

References are the relationship graph the UI uses to render breadcrumbs,
nested grids, and cross-entity aggregates. If you skip them, every cross-
entity feature in the UI becomes manual.

### Don't paper over backend differences in the model

A common temptation: declare a column as `i64` in your model because that's
what SQLite uses, even though the SurrealDB version stores it as a
floating-point. Then write custom conversion logic to bridge them.

Don't. Either:

- Use a custom type that implements both `SqliteType` and `SurrealType` with
  the right marker for each. `bakery_model3::Animal` is the pattern.
- Define two different field types, one per model variant, and let the UI
  read the right one.

When Diorama caches data, it round-trips through CBOR via the column type
system. Conversion bugs at the model level become cache bugs at the Diorama
level, which become "the UI sometimes shows the wrong number" bugs that are
miserable to debug. Get the types right at the bottom.

## What Diorama doesn't ask of your model

A few things you might worry about but shouldn't.

### No "Diorama-compatible" derive macro

Your `#[entity]` derive is enough. Diorama doesn't need additional traits,
markers, or capability declarations on the entity itself — all that lives at
the Vista level, which sits between your model and Diorama.

### No special id types

`String` ids work. `i64` ids work. UUIDs, ObjectIds, composite keys — all
fine, as long as the type implements the same persistence-side traits as any
other column. Diorama treats ids opaquely (as bytes for cache keys, as
typed values for record identity).

### No "dirty tracking" at the model level

The `Record<CborValue>` type that flows through Diorama doesn't currently
track which fields are dirty. When the UI patterns evolve to do
field-level edits (form changes one of twenty fields and only sends the
changed one), that capability lives in a wrapper around `Record` —
`EnrichedRecord` in Scenery contexts — not in your entity types. Your model
stays simple; the editing layer wraps it.

### No "this entity has 47 instances" constraint

Diorama's cache scales to as many or as few records as the backing source
holds. Your model doesn't declare expected cardinality. If a `Bakery` table
has 5 rows and a `Product` table has 5 million, both work — the difference
shows up in how the consumer configures the Lens for each (eager load for
small reference data, lazy fetch for large catalogs).

## A worked example — adding a new entity

Suppose `bakery_model3` grows a `Review` entity. The model code:

```rust
use vantage_sql::{Sqlite, SqliteType};
use vantage_table::Table;
use vantage_vista::flags;

#[entity(SqliteType, SurrealType, PostgresType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Review {
    pub rating: i64,
    pub body: String,
    pub author_name: String,
}

impl Review {
    pub fn sqlite_table(db: Sqlite) -> Table<Sqlite, Self> {
        Table::new("reviews", db)
            .with_id_column("id")
            .with_column(Column::new("rating"))
            .with_column(Column::new("body").with_flag(flags::SEARCHABLE))
            .with_column(Column::new("author_name").with_flag(flags::TITLE))
            .with_one("product", "product_id", crate::Product::sqlite_table)
    }
}
```

That's the entire model contribution. A consumer can now:

```rust
let reviews_dio = lens.make_dio(Review::sqlite_table(db).into_vista()?);

// CLI:
let recent = reviews_dio.vista().sort("created_at", Desc).list_values().await?;

// UI grid with quicksearch:
let grid = reviews_dio.table_scenery().sort("rating", Desc).open();

// Live counter of 5-star reviews:
let perfect_count = reviews_dio.value_scenery()
    .aggregate(Aggregate::count_where("rating", 5))
    .open();

// Cross-entity reference: opening a product's reviews:
let product_reviews = reviews_dio.vista()
    .add_condition_eq("product_id", product.id.clone())
    .into_dio_via(lens.clone());
```

The model author wrote one struct, one constructor, and a few column flags.
Every Diorama-using surface in the application gets cache-backed,
reactive, sortable, searchable Review views for free.
