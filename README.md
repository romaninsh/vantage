# Vantage

[![Book](https://github.com/romaninsh/vantage/actions/workflows/book.yaml/badge.svg)](https://romaninsh.github.io/vantage/)

Vantage is an **Entity Framework** for Rust. With Vantage you can represent your business entities
(Client, Order, Invoice, Lead) with native Rust types. Business logic implementation in Vantage and
Rust is type-safe and very ergonomic for large code-bases. Ideal for creating facade services,
middlewares and microservices or low-code backend UI.

Given your client record:

```rust
// Implemented in shared Business Model Library

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct Client {
    name: String,
    surname: Option<String>,
    gender: GenderEnum,
    email: Email(String),
    is_paying_client: bool,
    balance: Decimal,
}
```

Vantage offers the following features out of the box:

- [`DataSet<E>`](vantage-dataset/README.md) - Interface for a low-level persistence for an entity.
  There are some internal implementation like `CsvFile<E>` or `Queue<E>` which implement DataSet,
  but more importantly - you can use 3rd party implementations or build your own.

- [`ValueSet<V>`](vantage-dataset/README.md) - Interface for a uniform persistence without an
  entity. While this is very similar to strong-typed variant, ValueSet can use `serde_json::Value`
  or `ciborium::Value` to work with schema-less persistences. Example like `CsvFile` would implement
  `ReadableValueSet<String>` while `Queue` would implement `InsertableValueSet<Value>`.

- [`Table<E, D>`](vantage-table/README.md) - Struct for storing structured data with some columns.
  Can be used with NoSQL/SQL database engines, which implement interfaces for `TableSource`,
  `QuerySource` and `SelectSource`. Table auto-implements `DataSet<E>` and `ValueSet<D::Value>`.
- [`Expression<V>`](vantage-expressions/README.md) - Universal mechanism for constructing query
  builders. Expressions implement cross-database integration as well as define `Selectable`
  interface for minimalistic implementation of a query-builder (SQL-compatible).
- `ActiveEntity<E>` and `ActiveRecord<V>` - Implementation of ActiveRecord pattern, load the record,
  pass it around, modify and call `record.save()` when you are done.
- **Persistence-specific type systems** - Define vendor-specific type sets with
  [`vantage-types`](vantage-types/README.md) and create type-boundary systems preventing accidental
  casting (e.g., Duration to Int) while enabling precise database-native type mapping.

With all of the fundamental blocks and interfaces in place, Vantage can be extended in several ways.
First - persistence implementation:

- [`vantage-surrealdb`](vantage-surrealdb/README.md) - Implements all interfaces to interact with
  this powerful multi-modal database with support for custom types, schemaless tables, graph-based
  queries and live tables. Brings `SurrealSelect`, `SurrealColumn` types and also makes use of
  `SurrealType` that include Geospatial, 128-bit `Decimal` and other advanced types. Built on
  [`surreal-client`](surreal-client/README.md).

- `vantage-redb` - Implementation for Redb embedded database, providing some basic features. Perfect
  as a local cache.

On the other end, Vantage offers some adapters. Those would work with `Table` / `AnyTable` and
implement generic UI or API component:

- [`vantage-ui-adapters`](vantage-ui-adapters/README.md) - Implement DataGrid for Tauri, EGui,
  Cursive, GPUI, RatatUI and Slint frameworks.
- vantage-axum`*` - Implements builder for Axum supporting use of typed or generic tables.
- vantage-config - Reads Entity definitions from `yaml` file, creating type-erased `AnyTable`s.
- Vantage Admin`*` - Desktop Application for Entity management based on `yaml` config.

(`*` indicate commercial component)

Vantage `0.4` is almost here. New features include:

- **Full type system overhaul** - Complete redesign of the type mapping system for better database
  compatibility
- **Cross-database integration** - Seamless queries and operations across different database types
- **Binary CBOR support** - Migrated SurrealDB client from JSON to binary CBOR for improved
  performance
- References`(implemented)` allowing to traverse between `Table<Client>` into `Table<Order>`, even
  if those tables are stored in different databases.
- Expressions`(implemented)` allow Table columns to be calculated based on subqueries and
  references.

Features planned for `0.5`:

- **Hooks** - Attach 3rd party code plugins into Tables.
- **Validation** - Use standard and advanced validation through a hook mechanism.
- **Audit** - Capture modifications automatically and store in same or a different persistence.

Vantage is planning to add the following features before `1.0`:

- Aggregators. Use database-vendor specific extensions to build reports declaratively.
- GraphQL API adaptor. Build GraphQL APIs on top of Vantage Tables.
- Live Tables. Connect CDC or Live events provided by databases with UI adapters or WS APIs.

## Example

Vantage is an opinionated framework, which means it will provide guidance on how to describe your
business objects in Rust code. Here is a slightly expanded example, which describes how a Client
entity can be used with various persistences.

```rust
// Implemented in shared Business Model Library

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct Client {
    name: String,
    email: String,
    is_paying_client: bool,
    balance: Decimal,
}

impl Client {
    fn new(){ /* ... */};

    fn registration_queue() -> impl InsertableDataSet<Client> {}
    fn admin_api() -> impl DataSet<Client> {}
    fn read_csv(filename: String) -> impl ReadableDataSet<Client> {}
    fn mock() -> MockDataSet<Client> {}

    fn table() -> Table<SurrealDB, Client> {
        Table::new("client", surrealdb())
            .with_column_of::<bool>("is_paying_client")
            .with_many("orders", "client", move || Order::table(surrealdb()))
    }
}

// This is our way to implement Client Table specific features:
pub trait ClientTable {
    fn ref_orders(&self) -> Table<SurrealDB, crate::Order>;
}
impl ClientTable for Table<SurrealDB, Client> {
    fn ref_orders(&self) -> Table<SurrealDB, crate::Order> {
        self.get_ref_as("orders").unwrap()
    }
}
```

Definitions of your entities can be stored in your own crate, versioned and shared by many other
services within your organization. Here is how a typical service might make use of the Client
entity:

```rust
use model::Client;

async fn register_new_client(client: Client) -> Result<()> {
    let queue = Client::registration_queue(); // Probably KafkaTopic<Client>
    queue.insert(client).await?;
    Ok(())
}

async fn remove_stale_client_orders() -> Result<()> {
    let clients = Client::table()
        .with_condition(clients.is_paying_client().eq(false));

    // Delete orders of affected clients first (stored in MongoDB)
    clients.ref_orders().delete_all();

    // next delete clients (stored in SurrealDB)
    clients.delete_all();

    Ok(())
}
```

Enterprise software employs hundreds of developers, and anyone can make use of type safety without
knowing implementation details. Behind a simple interface of `clients` and `orders`, extensions of
Vantage allow embedding additional features:

- record any operations automatically into audit tables
- handle CDC for change tracking or perform necessary validations
- use multi-record operations efficiently, like in the case above - MongoDB will perform deletion
  with a single request.
- developer does not need to be mindful, where client table is stored.

Finally, thanks to the amazing Rust type system, the `client` type will not only implement
Client-specific extensions (like `ref_order()`) but will also implement SurrealDB extensions like
`search_query(q)`.

## Vantage features

A more comprehensive feature list:

- Standard CRUD operations with persistence-abstraction
- Implementation of DataSource-specific extensions
- Implementation of Entity-specific extensions
- Support for Expressions and Query Builder pattern (SQL, SurrealDB)
- MultiModal and Json database support (Mongo, GraphQL, SurrealDB)
- Data Source abstractions (CSV, Excel, PubSub, APIs)
- DataSet building - conditions, query-based expressions and delayed condition lookup
- Sync Relationship/References traversal - one-many and many-many - including cross-persistence.
- Generic types for DataSet, Table and ActiveRecord over Entity
- Support for standard and extended types on table columns
- Column flags
- (coming in 0.3) Column mapping, validation
- Typed queries (known result type) and associated queries (can self-execute)
- Type-erased structs for all major traits
- Adaptors for UI frameworks, APIs (like axum)
- (planned in 0.3) Aggregation, Joins, Expressions.
- Yaml-based Entity configurator
- Powerful Error handling

## Type erasure support

Rust type system is amazing and it is at the core of all Vantage features. In some cases, we want to
erase types, for example if we require a generic type interface.

Vantage provides a full set of "Any\*" types, which can be used like this:

```rust
let clients = Client::admin_api(); // impl DataSet<Client>
let clients = AnyDataSet::new(clients); // AnyDataSet - types erased.

let entities: = vec![clients, orders, ..];
```

Once type is erased - you can store different entities in same data-structure, implement
cross-language SDKs. If you are interested in "Any\*" types, see Documentation. The rest of the
README will focus on fully typed primitives.

## DataSet operations

Crate `vantage-dataset` introduces ImTableSource, implementing in-memory DataSet implementation.

```rust
let in_memory_cache = ImDataSource::new();

let client_cache = ImTable::<User>::new(&in_memory_cache, "clients");
let clients.import(Client::read_csv("clients.csv")).await?;

// Basic loading operation - load one client record
let (id, client) = clients.load_some().await?;
// Next - delete it from memory
clients.delete_id(&id).await?;
// Insert it back
clients.insert_id(id, client).await?;
```

The above operation will work consistently with ANY data-set implementation. If you switch to
`let clients = Client::admin_api()`, the rest of your code remains unchanged, because both `ImTable`
and `admin_api()` implement `DataSet<>`.

## Type casting and Value Sets

Some record types can be incredibly complex. Vantage assumes that any entity can be represented
through various types. Next example may fetch only the "name" field from admin_api (if API supports
this):

```rust
struct MiniClient {
    name: String,
}
let just_name = Client::admin_api()
    .get_id_as::<MiniClient>(client_id)
    .await?
    .name;
```

DataSet has no mandatory fields. Sometimes you don't want to use types at all. For this situation,
Vantage has full support for `serde_json::Value`:

```rust
let just_name = Client::admin_api()
    .get_id_value(client_id)
    .await?
    .get("name")?;
```

If you want to learn more about `ValueSet` - you can find more info in the documentation.

## Table

`Table<>` type implemented in `vantage-table` crate brings a crucial type into Vantage:

```rust
impl Client {
    fn table() -> Table<Client, Oracle>;
}
impl Order {
    fn table() -> Table<Order, MongoDB>;
}
```

Lets start by looking at the core features of Table:

- Table has Columns
- Table can be filtered
- Table can be ordered
- Table can paginate

To maximize performance, Table will always try to use database capabilities to implement the above
features, and retrieve data once it's filtered, ordered and paginated.

```rust
let mut order = Order::table();
order.add_condition(order.is_deleted().eq(false));
order.set_order_by(order.created_at().desc());
order.set_pagination(Pagination::ipp(25));

// filters, sorts and paginates on-the-server
for (id, order) in order.get().await? {
    dbg!(order);
}
```

Table can be cloned and all the above methods support builder-pattern:

```rust
let orders = Order::table();
for (id, order) in Order::table()
  .with_condition(order.is_deleted().eq(false))
  .with_order_by(order.created_at().desc())
  .with_pagination(Pagination::ipp(25))
  .get().await?
{
    dbg!(order);
}
```

## TableSource

Previous example created `order` table like this:

```rust
let order: Table<Order, MongoDB>
```

If MongoDB implements `TableSource`, then Vantage will implement `DataSet<E>` for
`Table<E, _: TableSource>` automatically. In other words, any table is also a DataSet. The
`import()` method accepts `ReadableDataSet`, so we can conveniently pass a MongoDB Table as an
argument.

Let's populate our in-memory cache from MongoDB:

```rust
let mongo_orders = Order::table(); // Table<Order, MongoDB>
let mongo_orders = mongo_orders
    .with_condition(mongo_orders.is_deleted().eq(false));
let im_orders = ImTable::<Order>::new(&in_memory_cache, "orders");

im_orders.import(&mongo_orders).await?;
```

## SelectSource and Queries

Relational databases like SQL or SurrealDB use powerful query languages to perform even more
operations inside the database. `vantage-expressions` implements building blocks for Query Builders.
Crates `vantage-surrealdb` and `vantage-sql` provide implementations for Query Builders as well as
the ability to execute those queries:

```rust
impl SelectSource for SurrealDB {
    type Select<E: Entity> = SurrealSelect;

    fn select<E: Entity>(&self) -> Self::Select<E>;
    async fn execute_select<E: Entity>(&self, select: &Self::Select<E>) -> Result<Vec<E>>;
}

impl Selectable for SurrealSelect { .. }
```

Vantage understands that databases have different SQL dialects or even entirely different query
languages. As with other things, Vantage expects all Query Builders to implement a bare minimum:
`impl Selectable`.

In our example, the organization stores `Client` in SurrealDB, but that is not a database that most
developers are familiar with.

Vantage query builder interface breaches this gap. Here is how we can calculate
`SELECT SUM(balance) from client where is_paying_client = true` in SurrealQL:

```rust
let clients = Client::table();
let clients = clients.with_condition(clients.is_paying_client().eq(true));

// SurrealSelect<result::Rows>
let query = clients.select();

// SurrealReturn
let sum_query = query.as_sum(clients.balance());

// Preview query
println!("Sum query: {}", sum_query.preview());

// Execute query and Convert Value into Decimal
let sum: Decimal = sum_query.get().await?.into();
```

The code listed here will work just as well if the Client table were stored in Oracle. Vantage makes
it very easy for organizations to move entities between persistences without changing code and
without sacrificing performance.

## Expressions

While we look into queries, I should also explain Expressions. Implementing Query builder dialects
with Vantage is easy because you rely on the Expression engine. In Vantage, the expression engine is
composable and supports parameters:

```rust
let now = if Some(date) = cutoff_date {
    expr!("{}", date)
} else {
    expr!("now()")
}
let condition = expr!("expires_at < {}", now);
let delete = expr!("DELETE FROM clients WHERE {}", condition);

// Prints: DELETE FROM clients WHERE expires_at < now() -- 0 parameters
// or DELETE FROM clients WHERE expires_at < $1  ($1 = '2024-06-01T00:00:00Z' )
println!("Delete query: {}", delete.preview());
```

Other query languages will struggle with a variable number of parameters and will outright make it
impossible to compose expressions from chunks.

Vantage additionally supports **deferred expressions** - a way to create queries across multiple
databases without async code.

## Deferred Expressions

Suppose you have 2 databases. Both support queries, but otherwise incompatible. How do you construct
a query like this:

```sql
select sum(vat) from MYSQL.orders where order.country_code in (
    select country_code from POSTGRES.countries where is_eu = true
)
```

Without Vantage you would query PostgreSQL first, await, fetch, insert result into MySQL query,
fetch await etc.

Vantage allows you to build query `sync` then query/fetch `async`:

```rust
let eu_countires = expr!("SELECT country_code FROM countries WHERE is_eu = true");
let eu_countries = postgres.defer(&eu_countires);

let vat_sum = expr!("SELECT SUM(vat) FROM orders WHERE country_code IN {}", eu_countries);
let vat_sum = mysql.query(&vat_sum).await?; // <-- single await!
```

But why would you operate with expressions, when you have query builders?

```rust
let eu_countries = postgres
    .select()
    .with_source(expr!("countries"))
    .with_field("country_code".to_string());

let vat_sum = mysql
    .select()
    .with_source(expr!("orders"))
    .with_expression(expr!("SUM(vat)", Some("total_vat".to_string()))
    .with_condition(expr!("country_code in {}', postgrs.defer(&eu_countries)))
    .get().await?; // <-- single await!
```

Why is this important? Because `Table<>` can encapsulate the above logic, hiding it's implementation
away inside the data model, exposing the interface like this:

```rust
let eu_orders = Orders::table().only_eu_orders(); // add_condition(expr)
let vat_sum = eu_orders.select().as_sum(eu_orders.vat()).get().await?;
```

## Expressionable

In Vantage, many different things can be part of an Expression. You already have seen dates and
whatever `defer()` returns as part of expressions. Any struct that implements Expressionable can be,
and this includes:

- Table columns: `eu_orders.vat()`
- Operations: `clients.is_paying_client().eq(true)`
- Sort order: `clients.name().desc()`
- Queries: `other_table.select()`
- Query builder components: `Identifier`, `Field`, `JoinQuery`, `SurrealReturn` or `Thing`
- Closure - that's what `defer()` returns after all.
- Scalar values - int, string, etc - those become parameters

Of course, you can implement more types and even your own unique Expression engines, making them
compatible. For example, MongoDB has an expression engine that results in JSON strings.

## Advanced Query Building

> WARNING: Vantage 0.2 code may be incompatible with 0.3

In Vantage, query builders implement the `Selectable` trait to make it familiar to developers.
However, SQL query builders are much more powerful and allow building any query using Rust:

```rust

let github_authors_and_teams = Query::new()
    .with_table("dx_teams", Some("t".to_string()))
    .with_field("team_source_id".to_string(), expr!("t.source_id"));

// Team is an anchestor
let github_authors_and_teams = github_authors_and_teams.with_join(query::JoinQuery::new(
    query::JoinType::Inner,
    query::QuerySource::Table("dx_team_hierarchies".to_string(), Some("h".to_string())),
    query::QueryConditions::on().with_condition(expr!("t.id = h.ancestor_id")),
));

// to a user with `user_name`
let github_authors_and_teams = github_authors_and_teams
    .with_join(query::JoinQuery::new(
        query::JoinType::Inner,
        query::QuerySource::Table("dx_users".to_string(), Some("dxu".to_string())),
        query::QueryConditions::on().with_condition(expr!("h.descendant_id = dxu.team_id")),
    ))
    .with_field("user_name".to_string(), expr!("dxu.name"))
    .with_field("github_username".to_string(), expr!("dxu.source_id"));
```

(Full example:
<https://github.com/romaninsh/vantage/blob/main/bakery_model/examples/3-query-builder.rs>)

## Table References

Vantage does not use term "relations" and instead uses "references". Defined like this:

```rust
let client = Client::table();
let client = client.with_many("orders", "client_id", || Box::new(Order::table()));

pub trait ClientTable {
    fn ref_orders(&self) -> Table<Order, SurrealDB> {
        self.get_ref_as("orders").unwrap()
    }
}
impl ClientTable for Table<Client, SurrealDB> {}
```

relationships can be traversed, transforming one table into another. In Vantage, traversal is also
`sync` and will just modify conditions:

```rust
let client_john = client.clone().with_condition(client.name().eq("John Doe"));
let johns_orders = client_john.ref_orders();
// Table<Order, SurrealDB>
```

As you can probably guess, Vantage allows you to traverse references across persistences as well,
and that happens transparently without changes to the model API.

Reference methods do not necessarily have to return Table; they can also respond with DataSet or
even something more specific like ReadableDataSet.

(Idea: create ReadableDataSet on top of arbitrary Select query - just need to add type)

## Associated Records

Up to this point, we have mostly looked at DataSets and Tables. They represent multiple records, but
sometimes you want to operate on a single record.

For this purpose, Vantage has yet another type: ActiveEntity<> and the ActiveEntitySet trait. The
standard table implementation already implements this trait:

```rust
let john_table = client.clone().with_condition(client.name().eq("John Doe"));

// ActiveEntity<Client, Table<Client, SurrealDB>>
let mut john = john_table.get_some_entity();
john.email = "john@example.com";
john.save().await?;
```

With ActiveEntity, you don't need to store ID. ActiveEntity has all the methods you implement for
Client, and in addition offers `id()` and `save()`.

ActiveEntity must not outlive a `WritableDataSet` where it must save itself, but you have some
amazing flexibility with this. For instance you can load entity from cache, but save it into
persistent table.

## Mock Testing

Testing business logic is inherently difficult. As a result - business logic test is done in the
**integration tests**, relies on database snapshots and is very slow.

Vantage introduces mocks at the SDK level and therefore business logic can be tested at **unit
test** level, without any external dependencies. This is much faster and will speed up your CI
pipelines, making engineers more productive.

## UI and API Adaptors

Vantage has a crate `vantage-ui-adapters` which has a reference integration with 6 different Rust UI
frameworks:

- Cursive
- EGui
- GPUI
- RatatUI
- Slint
- Tauri

The goal of adapters is to create a UI Table around a Vantage Table. Similarly, there is integration
with Axum (see `bakery_api` crate), which can be a great example of building generic REST APIs for
your `DataSets`. It should also be possible to implement a more sophisticated API such as GraphQL
API for Tables (but that would be a 3rd party crate).

## Table Columns

Table Columns type is defined by TableSource, so for SurrealDB Vantage uses SurrealColumn. This
technically allows to have Vendor-specific Column extensions.

Additionally Columns support flags, which is a feature aimed at generic UI builders. For further
information on this - check Any\* documentation.

## Using Vantage with Axum

Vantage fits well into Axum helping you build API handlers:

```rust
async fn list_orders(
    client: axum::extract::Query<OrderRequest>,
    pager: axum::extract::Query<Pagination>,
) -> impl IntoResponse {
    let orders = Client::table()
        .with_id(client.client_id.into())
        .ref_orders();

    let mut query = orders.query();

    // Tweak the query to include pagination
    query.add_limit(Some(pager.per_page));
    if pager.page > 0 {
        query.add_skip(Some(pager.per_page * pager.page));
    }

    // Actual query happens here!
    Json(query.get().await.unwrap())
}
```

API response for `GET /orders?client_id=2&page=1`

```json
[
  { "client_id": 2, "client_name": "Doc Brown", "id": 2, "total": 220 },
  { "client_id": 2, "client_name": "Doc Brown", "id": 3, "total": 995 }
]
```

## What's Coming in v0.5

Work-in-progress features for the next release:

- [`vantage-sql`](vantage-sql/README.md) - Implementation utilising SQLx and providing support for
  MySQL, PostgreSQL and SQLite.
- [`vantage-mongodb`](vantage-mongodb/README.md) - Implementation for MongoDB database.
- **Live Tables** - Mutexed tables with real-time updates and CDC integration.

## Roadmap

Vantage needs a bit more work. Large number of features is already implemented, but some notable
features are still missing:

- Joins - were present in 0.2, but not yet implemented in 0.4 (read and write)
- Aggregate columns - were in 0.2, but not yet implemented in 0.4
- Column hooks - allowing field mappings and custom calculation is still TODO.
- Cleanups - still missing some Any\* types.
- Interface consistency - more methods must return (id, E) tuple.
- SQL testing - most tests so far were done with SurrealDB. We should give SQL more love.
- Type support for SQL
- Oracle, because why not!
- Graph relations - implement hasMany support for Graph databases
- Consider Neo4j
- Implement some RestAPI adaptors (e.g. GitLab)
- Aggregators (grouping queries) for SQL and SurrealDB

## Installation

// For 0.2 version: Just type: `cargo add vantage`

// For 0.3 clone this repository and specify path to a crate.

If you like what you see so far - reach out to me on BlueSky:
[nearly.guru](https://bsky.app/profile/nearly.guru)

## Current status

Vantage currently is in development. See [TODO](TODO.md) for the current status.

## Author

Vantage is implemented by **Romans Malinovskis**. To get in touch:

- <https://www.linkedin.com/in/romansmalinovskis>
- <https://bsky.app/profile/nearly.guru>
