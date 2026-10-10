# vantage-mongodb

MongoDB backend for the [Vantage](https://github.com/romaninsh/vantage) persistence framework. Uses the official `mongodb` crate and `bson` types natively — no SQL generation, no expression templates, no ORM mapping layer. Conditions are `doc!{}` documents, queries are MongoDB pipelines, and relationships resolve through deferred `$in` lookups.

## What Problem Does Vantage MongoDB Solve?

You've chosen MongoDB for its flexible documents and powerful query language. Now you need to build an application that reads, writes, and traverses relationships across collections — while keeping the door open to support other backends (PostgreSQL, SQLite, CSV) with the same entity definitions.

You could use the `mongodb` driver directly, but then your application logic is coupled to BSON documents everywhere. You could build a repository layer, but then you're reinventing typed records, condition composition, and relationship traversal from scratch.

Vantage MongoDB gives you the table-level abstraction — typed entities, conditions, relationships, aggregates — while staying native to MongoDB's document model. You write `doc! { "price": { "$gt": 100 } }`, not a SQL-flavoured DSL that gets translated. The full MongoDB query language is available because conditions *are* BSON documents.

## Conditions Are Documents

This is the fundamental difference from SQL backends. Where `vantage-sql` builds expression trees with templates like `"{} > {}"`, MongoDB uses native BSON documents:

```rust
let mut products = product_table(db.clone());

// Filter with standard MongoDB operators
products.add_condition(doc! { "price": { "$gt": 130 } });

// Combine conditions — they merge with $and automatically
products.add_condition(doc! { "is_deleted": false });

// Use $expr for field-to-field comparisons
products.add_condition(doc! { "$expr": { "$gt": ["$price", "$calories"] } });

let results = products.list().await?;
```

Multiple `add_condition` calls combine with `$and` semantics. Each condition is a `MongoCondition`, which can be:

- **`Doc`** — an immediate `bson::Document` filter
- **`Deferred`** — an async function that resolves to a document at query time (used by relationships)
- **`And`** — a list of conditions merged at resolution time

No expression parsing, no template rendering. The `doc!{}` you write is the filter MongoDB receives.

## Defining Tables

Tables are defined the same way as other Vantage backends — a struct with `#[entity]`, and a constructor that declares columns and relationships:

```rust
#[entity(MongoType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Client {
    name: String,
    email: String,
    is_paying_client: bool,
    bakery_id: String,
}

impl Client {
    fn mongo_table(db: MongoDB) -> Table<MongoDB, Client> {
        let db2 = db.clone();
        let db3 = db.clone();
        Table::new("client", db)
            .with_id_column("_id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", move || {
                Bakery::mongo_table(db2.clone())
            })
            .with_many("orders", "client_id", move || {
                Order::mongo_table(db3.clone())
            })
    }
}
```

A few things to notice:

**`.with_id_column("_id")`** — MongoDB uses `_id` as the primary key, not `id`. This matters for relationship traversal, which needs to know the source table's ID field.

**`#[entity(MongoType)]`** — the same entity struct can support multiple backends. Add `SqliteType`, `PostgresType` etc. to get table constructors for each.

**`.with_one` and `.with_many`** — relationship definitions are identical to SQL backends. The difference is in how they execute.

## Relationship Traversal

SQL backends resolve relationships with subqueries — `WHERE id IN (SELECT bakery_id FROM client WHERE ...)`. MongoDB can't do subqueries across collections, so Vantage takes a different approach: it fetches the foreign key values from the source collection, then builds a native `$in` filter on the target collection.

```rust
let mut clients = client_table(db.clone());
clients.add_condition(doc! { "is_paying_client": true });

// Traverse: paying clients -> their orders
let orders = clients
    .get_ref_as::<MongoDB, ClientOrder>("orders")
    .unwrap();

let order_list = orders.list().await?;
// Returns orders where client_id IN [ids of paying clients]
```

Under the hood, this calls `related_in_condition`, which:

1. Queries the `client` collection for `_id` values matching the current conditions
2. Builds `doc! { "client_id": { "$in": ["marty", "doc"] } }`
3. Adds that as a deferred condition on the `client_order` collection

The traversal is application-side, not database-side — but the API is identical to SQL backends. Code that uses `get_ref_as` works the same whether the backend is PostgreSQL, SQLite, or MongoDB.

The reverse direction works too:

```rust
let mut orders = order_table(db.clone());
orders.add_condition(doc! { "_id": "order1" });

// Traverse: order -> its client
let client = orders
    .get_ref_as::<MongoDB, Client>("client")
    .unwrap();

let client_list = client.list().await?;
assert_eq!(client_list.values().next().unwrap().name, "Marty McFly");
```

## IDs: ObjectId or String

MongoDB's `_id` field can be an ObjectId or a plain string. `MongoId` handles both:

```rust
// 24-char hex strings are parsed as ObjectId
let id: MongoId = "507f1f77bcf86cd799439011".parse().unwrap();

// Everything else stays as a string
let id: MongoId = "hill_valley".parse().unwrap();
```

When you insert a record with a string ID, it's stored as a string `_id`. When you let MongoDB generate the ID, you get an ObjectId back. Both work transparently with `get`, `delete`, and relationship traversal.

## The Type System

MongoDB uses BSON, not JSON. `AnyMongoType` wraps `bson::Bson` with variant tracking so the framework knows the difference between an `Int32` and an `Int64`, a `String` and an `ObjectId`:

```rust
let val = AnyMongoType::new(42i64);
assert_eq!(val.type_variant(), Some(MongoTypeVariants::Int64));
assert_eq!(val.try_get::<i64>(), Some(42));
assert_eq!(val.try_get::<String>(), None); // Type-safe: won't coerce
```

Supported BSON types: Null, Bool, Int32, Int64, Double, String, ObjectId, DateTime, Binary, Array, Document, Decimal128, Regex, Timestamp.

Values from the database come back as "untyped" — no variant marker — so `try_get` checks the BSON discriminant directly. Values you create with `AnyMongoType::new()` carry a variant marker for stricter checking.

## Aggregates

Count, sum, max, and min use MongoDB's aggregation pipeline:

```rust
let count = products.get_count().await?;

let price_col = db.create_column::<AnyMongoType>("price");
let total = db.get_table_sum(&products, &price_col).await?;
let max_price = db.get_table_max(&products, &price_col).await?;
```

Conditions on the table are applied as a `$match` stage before the aggregation. So `products.add_condition(doc! { "is_deleted": false })` followed by `get_count()` counts only active products.

## Search

Full-text search across columns uses `$regex` with case-insensitive matching:

```rust
let condition = db.search_table_condition(&products, "cupcake");
products.add_condition(condition);
// Generates: { "$or": [{ "name": { "$regex": "cupcake", "$options": "i" } }, ...] }
```

Each column in the table gets an `$or` branch, at its stored BSON path (the column alias). This is a simple substring search — for more sophisticated text search, use MongoDB's `$text` operator directly via `doc!{}`.

## Vista capabilities

With the `vista` feature, a MongoDB vista advertises:

- Always: count, order, search (over `SEARCHABLE` columns by BSON path), page, keyset `fetch_next`,
  `fetch_window`, operator filters (`can_filter_operators`).
- Unless it is a pipeline view (read-only): insert, update, delete, confined writes, and reference
  traversal (`can_traverse_to_record`, `can_traverse_to_set`, `can_traverse_in_columns` for dotted
  join columns via `$lookup`).
- `can_build_ref_via_script`: with the `rhai` feature.
- `can_subscribe` (change streams, `watch_vista`) and `can_import` (transactional
  `import_vista_values`): need a replica set or sharded cluster. The topology is probed on connect;
  `MongoVistaFactory::with_change_streams(true)` forces them on, `false` off. A standalone server
  advertises neither, and forcing them on there does not give it change streams. `MongoDB::connect`
  waits out server selection (30 s by default) when the server is unreachable, then reports
  `Topology::Unknown`.

`$lookup` joins are type-strict: a string foreign key does not join an `ObjectId` `_id`. Each hop
matches `localField` against `foreignField`. Correlated subqueries are not implemented:
`Table::get_subquery_*` panics on MongoDB (`related_correlated_condition` is `unimplemented!`).
Reference keys must be stored under their own field name: a reference whose foreign key, or the id
column it matches, is renamed (`field:`) or nested (`nested_path:`) is refused when the vista is
built, unless the reference carries a `mongo: { rhai: }` script. A typed
`table.select()` run through `execute_select` does not lower joined columns; `list_values` and the
vista do.

A spec-built vista searches only its `SEARCHABLE` columns. A target reached through `get_ref` or
`get_ref_target` is wrapped with `from_table`: it searches every column when none is flagged, and
does not run that target spec's `modify:` script or its reference scripts.

YAML column types include `object_id`, `datetime` (aliases `date`, `timestamp`) and `decimal`. Typed
columns parse written text back into ObjectId, DateTime or Decimal128, and reject text that doesn't parse.

## YAML Vistas: `pipeline:`, `rhai:` and `modify:`

With the `vista` feature, a YAML spec can make a vista read an aggregation pipeline instead of a collection. The result is read-only. Each row's id is the `_id` the pipeline emits (for a `$group`, the group key):

```yaml
name: category_totals
columns:
  _id: { type: string, flags: [id] }
  total: { type: int }
mongo:
  collection: products
  pipeline:
    - $match: { active: true }
    - $group: { _id: "$category", total: { $sum: "$price" } }
```

The `rhai` feature adds two script slots. `rhai:` builds the pipeline in Rhai, so it can depend on `args`, the parameters the host sets at open time. `rhai:` and `pipeline:` can't be used together. `modify:` runs last, against the built vista as `self`:

```yaml
mongo:
  collection: products
  rhai: |
    let stages = [];
    if "cat" in args { stages.push(#{"$match": #{category: args.cat}}); }
    stages.push(#{"$group": #{_id: "$category", n: #{"$sum": 1}}});
    stages
  modify: |
    self.with_condition(#{n: #{"$gt": 1}})
```

Maps convert straight to BSON. `oid("…")` builds an `ObjectId`, and `date("2026-01-01T00:00:00Z")` builds a BSON date. A reference can also be built by a script (`references: { orders: { …, mongo: { rhai: 'table("order").where("client_id", me._id)' } } }`). This is how the rows of a `$group` view lead to other tables.

**Key order.** YAML `pipeline:` keeps keys in the order you wrote them. Rhai map literals sort their keys, so `#{b: 1, a: -1}` reaches MongoDB as `{a: -1, b: 1}`. Most of the time this doesn't matter: `$match` filters, `$group` fields and `$project` fields ignore key order. Two cases do care:

- A multi-key `$sort`, or `$setWindowFields`' `sortBy`, sorts by key order. Build it with `doc([["b", 1], ["a", -1]])`, which keeps the order given.
- An exact match on an embedded document compares key order too. Match nested fields by dotted path instead: `#{"address.city": "Riga"}`, not `#{address: #{city: "Riga"}}`.

**Missing args.** If the host didn't set `args.x`, it reads as `()`, which becomes BSON `null`, and the filter then matches only documents where the field is null or absent. Guard optional args with `"x" in args`.

## Multi-Backend Applications

Because `vantage-mongodb` implements the same `TableSource` trait as every other backend, your entities can have constructors for multiple backends:

```rust
// Same entity, different backends
let pg_products = Product::postgres_table(pg_db);
let mongo_products = Product::mongo_table(mongo_db);
let csv_products = Product::csv_table(csv);

// Type-erased — works with any backend
let any_table = AnyTable::from_table(Product::mongo_table(db));
let records = any_table.list_values().await?;
```

The `bakery_model3` example in the Vantage repo demonstrates a CLI tool that queries the same entities across CSV, SQLite, PostgreSQL, SurrealDB, and MongoDB — choosing the backend at runtime.

## Beyond CRUD

The `MongoDB` struct exposes the underlying driver for anything Vantage doesn't abstract:

```rust
let collection = db.collection::<bson::Document>("products");

// Use the driver directly for operations Vantage doesn't abstract.
let pipeline = vec![
    doc! { "$unwind": "$tags" },
    doc! { "$group": { "_id": "$tags", "count": { "$sum": 1 } } },
];
let cursor = collection.aggregate(pipeline).await?;
```

Vantage MongoDB handles typed CRUD, conditions, relationships and aggregates. Anything else goes through the driver handle.
