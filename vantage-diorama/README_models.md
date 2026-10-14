# Defining Models for Diorama

This is for someone writing a model crate like `bakery_model3`: typed entities,
a table constructor per backend, references between them. Nothing in your model
has to know that Diorama exists. You write it as you would for plain Vista use,
and Diorama wraps the resulting Vista without changing the contract.

A few choices you make in the model do change how well a Diorama-backed UI or
cache behaves around it. This document covers those, and then what a consumer
builds on top of a model: narrowed Dios and aggregates.

## The pattern that already works

`bakery_model3` is the reference:

```rust
#[entity(CsvType, SurrealType, SqliteType, PostgresType, MongoType, DynamoType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Product {
    pub name: String,
    pub calories: i64,
    pub price: i64,
    pub is_deleted: bool,
    pub sticker: Option<Animal>,
}

impl Product {
    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<Option<Animal>>("sticker")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", Bakery::sqlite_table)
    }
}
```

A `Table` becomes a `Vista` through its backend's factory, and a `Vista`
becomes a Dio through a Lens:

```rust
let products = db.vista_factory().from_table(Product::sqlite_table(db.clone()))?;
let dio = lens.dio(products).build().await?;
```

Neither step asks anything of the model beyond a well-described table.

## What metadata helps

The factory turns the table's columns, id, titles, flags and references into
the Vista's metadata, and that metadata is all Diorama reads. A few parts of it
carry most of the weight.

### Declare the id column

`with_id_column("id")` is what makes a record addressable. A Dio needs it:
`lens.dio(vista).build()` fails with "a Dio needs a vista with an id column"
when the Vista declares none. Diorama keys by it:

- the cache row (one id, one stored record);
- `dio.record_scenery(id)` and `dio.servo(id)`;
- every event on the bus (`RecordChanged { id }` and the rest);
- the flash in flight for a row, which a refresh leaves alone until it settles.

When your id isn't called `id` (an account number, a SKU, Mongo's `_id`),
declare it by its name:

```rust
Table::new("products", db).with_id_column("sku")
```

### Declare column types

Typed columns (`with_column_of::<i64>("price")`) are more than documentation.
Two things read the declared type:

- **The cache round trip.** Rows are cached as CBOR. A column the model types
  as `i64` in SQLite and stores as a float in SurrealDB comes back as two
  different values, and "the grid sometimes shows the wrong number" is a
  miserable bug to trace back to a model. Use a type that implements both
  backends' markers (`bakery_model3::Animal` is the pattern), or give each
  backend its own field type.
- **Aggregate conditions.** A `Where` term converts its operand to the type
  its column declares before comparing, so a configured `code: "200"` matches
  an integer column. A column with no declared type compares the operand as
  given. See [Aggregates](#aggregates).

### Flag the columns whose role isn't obvious

The flags a UI and the in-memory evaluator care about:

- **Title.** `with_title_column_of::<String>("name")` marks the column shown
  when the record is referenced from elsewhere: a dropdown's label, the
  "Customer: John Smith" an order row shows. A `titles_only()` picker reads
  these columns.
- **Searchable.** A quicksearch that Diorama evaluates itself (an eager view,
  a whole copy) scans the columns flagged `Searchable`, or every column when
  none is. Flag the few that should match, so a search for "12" doesn't match
  every price.
- **Orderable.** The SQL factories mark every column orderable; other drivers
  mark what their engine can sort. The facade Vista from `dio.cached()` treats
  every column as orderable, because it sorts what it reads when the master
  can't.

```rust
Table::new("products", db)
    .with_id_column("id")
    .with_title_column(Column::<String>::new("name").with_flag(ColumnFlag::Searchable))
    .with_column(Column::<String>::new("description").with_flag(ColumnFlag::Searchable))
    .with_column_of::<i64>("price")
```

Flag only what the type and name don't already say.

### Declare references

`with_one` and `with_many` describe the graph a UI walks. A Dio follows them:

- `dio.get_ref("customer", &row).await?` is a new Dio over the related rows of
  one row, on the same Lens, with a cache table of its own.
- `dio.get_ref_target("customer").await?` is the relation's whole target: what
  a reference picker lists, or where a new related row goes.

Skip the references and every cross-entity screen (breadcrumbs, nested grids,
pickers, "47 open orders") has to be wired by hand.

## What Diorama doesn't ask of your model

- **No Diorama derive.** `#[entity]` is enough. What Diorama needs lives at
  the Vista level, between your model and the Dio.
- **No special id type.** `String`, `i64`, UUID or ObjectId ids all work.
  Diorama keys rows by the id's text.
- **No dirty tracking.** Records flow through Diorama as `Record<CborValue>`.
  A form's edits live in a `Servo`, which diffs the draft against the record
  and sends only the changed fields. Your entity stays plain.
- **No cardinality.** A 5-row bakery table and a 5-million-row product table
  use the same model code. The difference is the Lens the consumer picks for
  each: an eager copy for small reference data, paged windows for a large
  catalog.

## Narrowing a Dio

A built Dio never changes. To narrow one, derive a new Dio from it:

```rust
let reviews_of_product = reviews
    .derive()
    .with_condition(Condition::new("product_id", FilterOp::Eq, product_id))
    .with_order("rating", SortDirection::Descending)
    .build()
    .await?;
```

The master takes the clauses it can; the rest become the derived Dio's
`residual()`, which Diorama evaluates over the rows it caches. Every read,
view, aggregate and write of the derived Dio stays inside its set: a write that
would land a row outside it is refused before anything is staged. A search or
order given to `derive()` replaces the parent's.

## Aggregates

An aggregation reduces the rows a Dio admits (its residual applied) and is
recomputed as they change. It mounts two ways:

- **As a value:** `dio.value_scenery(aggregation).open().await?` for an
  aggregation whose output is one CBOR value. This is what a counter or badge
  binds to.
- **As a table:** `dio.aggregate(aggregation).await?` for one whose output is
  rows. It returns a new Dio, read-only, which a grid binds to like any other.

The building blocks live in `vantage_diorama::aggregate`:

| Aggregation | Output |
|---|---|
| `Count::rows()` | the number of rows |
| `Sum::of(col)`, `Avg::of(col)`, `Min::of(col)`, `Max::of(col)` | a number over one column |
| `Distinct::of(col)` | the number of distinct values |
| `Where::new(conditions, inner)` | `inner` over the rows `conditions` admit |
| `GroupBy::column(col, reducer)` | one row per value of `col` (a table) |
| `AsRows::new(col, scalar)` | a scalar as a one-row, one-column table |

```rust
use vantage_diorama::aggregate::{AsRows, Conditions, Count, GroupBy, Sum, Where};

let revenue = orders.value_scenery(Sum::of("amount")).open().await?;

let open = orders
    .value_scenery(Where::new(Conditions::new().eq("status", "open"), Count::rows()))
    .open()
    .await?;

let by_status = orders.aggregate(GroupBy::column("status", revenue_per_group)).await?;
let grid = by_status
    .table_scenery()
    .order("revenue", SortDirection::Descending)
    .open()
    .await?;

let total = orders.aggregate(AsRows::new("n", Count::rows())).await?;
```

`revenue_per_group` is a `Reduce`: the columns it adds, and a closure from the
group's key and rows to one record. A `Sum` over an integer column is an
integer and over a fractional one a float. An empty source is a real answer: a
sum of zero, a table with no rows.

### Why recompute instead of updating incrementally?

An incremental fold has to retract a row, and `max`, `min` and `distinct` can't
without a rebuild. Recomputing the whole aggregation on every change has
nothing to drift and nothing to retract, so those need no special case. Two
things keep the cost down: a burst of changes collapses into one recomputation
and one trailing flush (`Tuning::aggregate_debounce`, on the source Dio's
Lens), and an output equal to the last one publishes nothing, so recomputing
often doesn't mean repainting often.

The cost is real on one shape. An aggregation declares what it reads. `Count`
reads no column, so an unnarrowed count comes straight from the cache's count.
`Sum::of(col)` reads one column. A `Reduce` closure can read anything, so it
reads `Reads::Rows`, and on an augmented Dio that means every cached row is
hydrated before each recompute. On an augmented source, prefer the built-ins
(a `Where` over one reads its terms' columns too); for a grouped table,
`AggregationCatalog::group_by` builds a reducer out of built-ins that declares
the columns it reads.

### Where conditions

`Conditions::new().eq(col, value)` adds an equality term; terms are ANDed.
Before matching, each operand is converted to the type its column declares:
text parsed as an integer, a float or a bool, a scalar rendered as text. A
configuration file can't know the backend's types, and a filter that silently
matched nothing would be a wrong answer rather than an error. Exact decimals
are never converted, since parsing `"0.1"` through a float compares a
different number. A term naming a column the row doesn't have never matches;
a typo must not widen the aggregate to every row.

### What a derived Dio is

`dio.aggregate(..)` mounts a new Dio on a Lens of its own, in memory, on the
source's runtime and debug stream. Its Vista holds the whole output, so it
advertises `Order`, `Search` and `Window` and honours them; sorting a derived
table is ordinary push-down into its own Vista. It can't be written to: its
rows are a function of the source.

Every aggregate of one source is named `<source>#aggregate`, whatever the
aggregation. Two aggregates of one source are still two Dios with their own
rows, but the debug stream and the diagnostics can't tell them apart by name.
The returned Dio is the only handle: dropping every clone stops the
recomputation and releases the source.

## Adding a new entity

Suppose `bakery_model3` grows a `Review`:

```rust
#[entity(SqliteType, SurrealType, PostgresType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Review {
    pub rating: i64,
    pub body: String,
    pub author_name: String,
}

impl Review {
    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Review> {
        Table::new("review", db)
            .with_id_column("id")
            .with_column_of::<i64>("rating")
            .with_column(Column::<String>::new("body").with_flag(ColumnFlag::Searchable))
            .with_title_column_of::<String>("author_name")
            .with_column_of::<String>("product_id")
            .with_one("product", "product_id", Product::sqlite_table)
    }
}
```

That is the whole model contribution. A consumer can now:

```rust
let vista = db.vista_factory().from_table(Review::sqlite_table(db.clone()))?;
let reviews = lens.dio(vista).build().await?;

// A grid, sorted, searchable over `body`:
let grid = reviews
    .table_scenery()
    .order("rating", SortDirection::Descending)
    .open()
    .await?;

// A live count of five-star reviews:
let five_star = reviews
    .value_scenery(Where::new(Conditions::new().eq("rating", 5i64), Count::rows()))
    .open()
    .await?;

// One product's reviews, as a Dio of their own:
let for_product = reviews
    .derive()
    .with_condition(Condition::new("product_id", FilterOp::Eq, product_id))
    .build()
    .await?;
let rows = for_product.cached().list_values().await?;
```

## Checklist

- Declare the id column; a Dio won't build without one.
- Type every column, and type it the same way the backend stores it.
- Mark title columns, and flag the columns a quicksearch should match.
- Declare `with_one` and `with_many` references.
- Leave caching, cardinality and editing state out of the model; the Lens, the
  views and the `Servo` own them.
