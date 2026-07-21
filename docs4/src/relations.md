# Relations & Traversal

The bakery model has three tables that belong together: a `Bakery` has many `Clients`, and each
`Client` has many `Orders`. In SQL terms, `client.bakery_id` points at a bakery and
`client_order.client_id` points at a client. Almost every real question you'd ask of this data
crosses one of those links — "orders of paying clients", "which bakery does this order belong to" —
so this guide is about how Vantage models those links and how you cross them.

You met relations briefly in the [Introduction](intro/step2-tables.md), where a product catalog
picked up `with_many` and `get_ref_as`; this guide covers the same machinery in depth, with a fuller
model. We work SQL-first here (SQLite, using the `bakery_model3` crate), with SurrealDB and other
backends in notes along the way. The chapters after this one dig into traversal forms, subquery
expressions, implicit references, and how relations survive into the type-erased `Vista` and cached
`Dio` layers — this page sets up the vocabulary they all share.

## What a relation is

A relation in Vantage is a declared, named link between two table definitions. It lives on the
**table definition** — the model — not on the entity struct, and not in the database. Vantage never
introspects foreign-key constraints; whether your SQLite schema actually declares
`FOREIGN KEY (client_id) REFERENCES client(id)` is invisible to it. A relation is exactly three
things:

1. **A name** — the string you traverse by, like `"orders"`.
2. **A foreign-key field** — the column that carries the link.
3. **A target-table constructor** — a function that builds the table on the other end.

### Why not read the foreign keys from the database?

Because most of the backends Vantage talks to don't have any. CSV files, REST APIs, and MongoDB
have no foreign-key concept to introspect, yet their data is just as related. Declaring the
relation on the model means the same declaration works everywhere — the link between clients and
orders exists whether the rows live in Postgres or in a spreadsheet export.

## Two cardinalities

There are two declaration methods, and the difference between them comes down to one question:
which table holds the foreign key?

- **`with_one("name", "fk_field", constructor)`** — the FK lives on **this** table. Traversal
  yields at most one row: many-to-one. An order has one client.
- **`with_many("name", "fk_field", constructor)`** — the FK lives on the **target** table.
  Traversal yields a set: one-to-many. A client has many orders.

Internally these become `HasOne` and `HasMany` implementations of the `Reference` trait
(`vantage-table/src/references/`), and the cardinality is surfaced as `ReferenceKind::HasOne` /
`ReferenceKind::HasMany` for anything that needs to reason about relations generically — the vista
layer will, later.

## Declaring relations on the model

Here is `Client` from `bakery_model3/src/client.rs` — the entity struct and its SQLite table
constructor:

```rust
#[entity(CsvType, SurrealType, SqliteType, PostgresType, MongoType, DynamoType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Client {
    pub name: String,
    pub email: String,
    pub contact_details: String,
    pub is_paying_client: bool,
    pub bakery_id: Option<String>,
}

impl Client {
    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Client> {
        Table::new("client", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", Bakery::sqlite_table)
            .with_many("orders", "client_id", Order::sqlite_table)
    }
}
```

Notice that the struct knows nothing about relations. `bakery_id` is just an `Option<String>` field
like any other; the links are declared on the table definition, where the columns and conditions
already live.

And here is the other end, from `order.rs`:

```rust
impl Order {
    pub fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Order> {
        Table::new("client_order", db)
            .with_id_column("id")
            .with_column_of::<String>("client_id")
            .with_column_of::<bool>("is_deleted")
            .with_one("client", "client_id", Client::sqlite_table)
    }
}
```

The same underlying relation appears from both ends: `Client` declares
`with_many("orders", "client_id", …)` and `Order` declares `with_one("client", "client_id", …)`.
Each side is declared independently and each is usable independently — you could declare only one
of them if you never traverse the other direction. And both name the same column: `"client_id"` on
the `client_order` table. For `with_many` that's the FK on the *target* table; for `with_one` it's
the FK on the *source* — here `Order` happens to be the FK-carrying side both times.

### The constructor argument

The third argument — `Order::sqlite_table` — is not a table, it's a function. It's the very same
constructor the model already exposes; you're just passing it by name instead of calling it. That
buys two things. First, declaring a relation costs nothing at build time: the target table is only
constructed when the relation is actually traversed. Second, and more importantly, when the target
*is* constructed, it's the real model definition — its own columns, its own conditions, its own
relations. If `Order::sqlite_table` filtered out soft-deleted rows, every traversal that reaches
orders through a relation would inherit that filter. There is no second, weaker definition of
"orders" that relations quietly use.

## A first traversal

Traversal is set-to-set: narrow the source set, traverse, and the target set arrives already
narrowed by the relationship. Here's "orders of paying clients":

```rust
let mut paying = Client::sqlite_table(db.clone());
paying.add_condition(paying["is_paying_client"].eq(true));

// Table<SqliteDB, Order>, narrowed to orders of paying clients
let orders = paying.get_ref_as::<Order>("orders")?;
```

No query has run yet — `orders` is a `Table<SqliteDB, Order>` like any other, and you can add
conditions, select columns, or traverse further. When it does execute, the narrowing happens via a
subquery — not a JOIN, and not a round-trip to fetch client IDs first. The shape (illustrative):

```sql
SELECT ... FROM "client_order"
WHERE "client_id" IN (SELECT "id" FROM "client" WHERE "is_paying_client" = 1)
```

This is the payoff of set-to-set thinking: "orders of paying clients" is one narrowing plus one
traversal, executed as a single query — never a loop over IDs. The next chapter covers the full
range of traversal forms; this is just the first taste.

### Giving traversals a name

`get_ref_as::<Order>("orders")` works, but strings and turbofish don't belong in application code.
The model crate wraps its relations in an extension trait:

```rust
pub trait ClientTable {
    fn ref_orders(&self) -> Table<SqliteDB, Order>;
}

impl ClientTable for Table<SqliteDB, Client> {
    fn ref_orders(&self) -> Table<SqliteDB, Order> {
        self.get_ref_as("orders").unwrap()
    }
}
```

The `unwrap()` is safe here: `"orders"` is declared on every `Client` table the model produces, so
a typo panics in development, not silently at runtime. Callers just write `clients.ref_orders()` —
no turbofish, no string. This is the same pattern the [Introduction](intro/step2-tables.md) used,
and [Model-Driven Architecture](mda.md) covers where these traits live in a model crate.

## SurrealDB: record links instead of foreign keys

SurrealDB stores relations as **record links** — the FK column holds a typed record id (a `Thing`,
rendered `table:key`) rather than a scalar. The declaration shape is identical; only the field
differs. From the same `bakery_model3/src/client.rs`:

```rust
pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Client> {
    Table::new("client", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("email")
        .with_column_of::<String>("contact_details")
        .with_column_of::<bool>("is_paying_client")
        .with_one("bakery", "bakery", Bakery::surreal_table)
        .with_many("orders", "client", Order::surreal_table)
}
```

The link column is conventionally named after the target — `"bakery"`, not `"bakery_id"` — and
the relation *name* is free to differ from the link field; nothing couples them.

```admonish info title="Other backends"
MongoDB, CSV, and DynamoDB declare relations exactly the same way, with scalar foreign keys —
`bakery_model3` has `mongo_table`, `csv_table`, and `dynamo_table` constructors of the same shape.
Declaring works everywhere; what differs per backend is which *traversal forms* it can push down,
which the next chapter covers.
```

## Beyond sets

Relations don't stop at set-to-set. When you hold a single row or a loaded record, traversal works
from there too — and it cooperates with the record lifecycle: inserting through a traversed set
auto-fills the foreign key, so an order created via `client.ref_orders()` already belongs to that
client. The details live in [Records: Traversal, Invariants & Hooks](record-lifecycle.md).

## Where this guide goes

You now have the vocabulary: a relation is a named `(name, fk_field, constructor)` triple on the
table definition, in one of two cardinalities, traversed set-to-set. The child chapters each take
one thread further:

- **[Traversing Sets and Records](relations/traversal.md)** — the full range of traversal forms:
  set-to-set, row-in-hand, record-level, and contained.
- **[Expressions & Subqueries](relations/expressions.md)** — correlated lookups, counts, and how
  relation-derived expressions compose.
- **[Implicit References](relations/implicit-references.md)** — dotted column names, the
  declarative way to pull a field across a relation.
- **[Relations on Vistas](relations/vistas.md)** — traversal after type erasure, capability
  checks, and declaring relations in YAML.
- **[Relations and Dio](relations/dio.md)** — where cross-datasource enrichment lives.
