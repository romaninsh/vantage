# Vantage

[![Book](https://github.com/romaninsh/vantage/actions/workflows/book.yaml/badge.svg)](https://romaninsh.github.io/vantage/)

Vantage is an **Entity framework** for Rust apps that implements an opinionated Model
Driven Architecture.

Vantage makes Rust more suitable for writing Business software such as CRM, HR, ERP or Low Code apps
where large number of entities (types representing business objects, like an 'Invoice') must hold
complex relationship, attribute, validation and other business rules.

Vantage framework focuses on the following 3 areas:

- **Entity definition** - Using Rust code, describe your logical business entities,
  their attributes, relationships and business rules.
- **Query Building** - Dynamically create SQL queries using SQL dialect of your choice
  and utilise full range of database features. Queries are not strictly SQL - they can
  be implemented for NoSQL databases, REST APIs or even GraphQL.
- **Data Sets** - Implementation of a type that represents a set of records, stored
  remotely. Data Sets can be filtered, joined, aggregated and manipulated in various ways.
  Most operations will yield a new DataSet or will build a Query incapsulating all
  the business logic.

It is important that all 3 parts are combined together and as a result - Vantage
allows you to write very low amount of code to achieve complex business logic without
sacrifising performance.

Vantage introduces a clean app architecture to your developer team, keeping them
efficient and your code maintainable.

## Defining Entities

While ORM libraries like Diesel or SQLx will use your SQL structure as a base, Vantage
allows you to define your entities entirely in Rust code, without boilerplate. You do not
need to keep your entities in sync with SQL schema. For example, consider the following
structure:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct Invoice {
    id: i64,
    client_name: String,
    total: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct InvoiceLine {
    id: i64,
    invoice_id: i64,
    product_code: String,
    quantity: i64,
    price: i64,
}

impl Entity for Invoice {}
impl Entity for InvoiceLine {}
```

Those structures are handy to use in Rust, but they do not map directly to SQL schema.
Fields `client_name`, `total` and `product_code` are behind joins and subqueries.
This is fully supported by Vantage - you can have several Rust structs for interfacing
with your business entities, depending on use-case.

```rust
let invoice: Invoice = Invoice::table().with_id(123).get_one().await?;
println!("Invoice: {:?}", invoice); // calculates total, client_name etc.
```

## Query Building

In pursuit of better performance developers of business apps often resort to writing
entire queries with `sqlx`. While this may work for a small application, for a large
project you would want generic types, dynamic queries and a better way to use Rust
autocomplete and type systems.

Vantage provides a way to express your SQL queries in native Rust - dynamically:

```rust
use vantage::prelude::*; // sql::Query

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

(Full example: <https://github.com/romaninsh/vantage/blob/main/bakery_model/examples/3-query-builder.rs>)

SQL is not the only query type supported by Vantage. You can use NoSQL queries, REST API
and GraphQL queries too. Each query type is unique but will implement some shared traits.

## Data Sets

The third concept introduced by Vantage is Data Sets. This allows you to create a generic
interface between your entities and query builder. This way you just need to define
what you want to do with your entities, and the query will be built for you automatically:

```rust
let clients: Table<Postgres, Client> = Client::table().with_condition(Client::is_paying_client().eq(&true));
let unpaid_invoices: Table<Postgres, Invoice> = clients.ref_invoices()
    .with_condition(Invoice::table().is_paid().eq(&false));

send_email_reminder(unpaid_invoices, "Pay now!").await?.unwrap();

// This can also use generics:
async fn send_email_reminder(data: impl ReadableDataSet<Invoice>, message: &str) -> Result<(), Error> {
    for invoice in data.get().await? {
        println!("Sending email to {} with message: {}", invoice.client_name, message);
    }
}
```

Let me walk you through the code above so we can trace how Vantage builds queries out of
entities for you:

- Client::table() returns Table<Postgres, Client> type, because that's where our clients
  are stored.
- with_condition() narrows down the set of clients to only those who are paying. Because
  Postgres supports `where` clause, this will become part of a `clients` table query.
- ref_invoices() returns Table<Postgres, Invoice>, which will be based on Invoice::table()
  but with additional conditions and client subquery.
- final with_condition() narrows down the set of invoices to only those that are unpaid.

Resulting type is `sql::Table<Postgres, Invoice>`. It has been mutated to accomodate all the
changes we made to it, but query was not executed yet.

Next we pass `unpaid_invoices` to `send_email_reminder` function, which would have accepted
anything that implements `ReadableDataSet<Invoice>`. To `send_email_reminder` it does not
matter if the data is coming from SQL, NoSQL or REST API. It only intends to fetch the
data at some point.

## Extensions and Plugins

`Table<D, E>` implements a number of other useful traits:

- `TableWithColumns` - allows you to describe table columns and map them to Rust types.
- `TableWithConditions` - allows you to add conditions to the query.
- `TableWithJoins` - allows you use 1-to-1 joins and store record data across multiple tables.
- `TableWithQueries` - allow you to build additional queries like sum() or count().

And of course you can add your own extensions to your table definitions:

```rust
impl MyTableWithACL for Table<_, MyEntity> {}
```

## Vantage and stateful applications

Many Rust application are stateful. Implementing a UI may include search field, filters,
pagination to limit amount of records you need to display for the user. There
may be a custom field selection and even custom column types that you would need to deal
with. Multiply that by 20-50 unique business entities, add all the UI you must build
along with ACL and validation rules.

Without generic UI components, this will be a nightmare to implement. Vantage can help
yet again.

`Table` can be kept in memory, shared through a Mutex or Signal, modified by various
UI components and provide `Query` to different parts of your application. For instance,
your paginator component will want to use `table.count()` to determine how many records
are there in total and use `table.set_limit()` to paginate resulting query. Your filter
form component would use `table.get_columns()` to determine
what fields are available for filtering and `table.add_condition()` to apply those
conditions. Your data grid component would use `table.get()` to fetch the data.

Rich data grid views are the core component of business applications and while Vantage
does not provide a UI, it can drive your generic components and provide both structure
and data for them.

## Quick Start

While not mandatory, I recommend you to define some entities before starting with Rust.
Provided [bakery_model](bakery_model/src/) implements entities for "Baker", "Client", "Product",
"Order" and "LineItem" - specifying fields and relationships, you may write business code
relying on auto-complete and Rust type system:

```rust
use vantage::prelude::*;
use bakery_model::*;

let set_of_clients = Client::table();   // Table<Postgres, Client>

let condition = set_of_clients.is_paying_client().eq(&true);  // condition: Condition
let paying_clients = set_of_clients.with_condition(condition);  // Table<Postgres, Client>

let orders = paying_clients.ref_orders();   // orders: Table<Postgres, Order>

for row in orders.get().await? {  // Order
    println!(
        "Ord #{} for client {} (id: {}) total: ${:.2}\n",
        order.id,
        order.client_name,
        order.client_id,
        order.total as f64 / 100.0
    );
};
```

Output:

```
Ord #1 for client Marty McFly (id: 1) total: $8.93
Ord #2 for client Doc Brown (id: 2) total: $2.20
Ord #3 for client Doc Brown (id: 2) total: $9.95
```

SQL generated by Vantage and executed:

```sql
SELECT id,
    (SELECT name FROM client WHERE client.id = ord.client_id) AS client_name,
    (SELECT SUM((SELECT price FROM product WHERE id = product_id) * quantity)
    FROM order_line WHERE order_line.order_id = ord.id) AS total
FROM ord
WHERE client_id IN (SELECT id FROM client WHERE is_paying_client = true)
  AND is_deleted = false;
```

This illustrates how Vantage combined specific rules of your code such as "only paying clients" with
the rules defined in the [bakery_model](bakery_model/src/), like "soft-delete enabled for Orders"
and "prices are actually stored in product table" and "order has multiple line items" to generate
a single and efficient SQL query.

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

Compare to [SQLx](https://github.com/launchbadge/realworld-axum-sqlx/blob/main/src/http/articles/listing.rs#L79), which is more readable?

## Key Features

- 🦀 **Rust-first Design** - Leverages Rust's type system for your business entities
- 🥰 **Complexity Abstraction** - Hide complexity away from your business logic
- 🚀 **High Performance** - Generates optimal SQL queries
- 🔧 **Zero Boilerplate** - No code generation or macro magic required
- 🧪 **Testing Ready** - First-class support for mocking and unit-testing
- 🔄 **Relationship Handling** - Elegant handling of table relationships and joins
- 📦 **Extensible** - Easy to add custom functionality and non-SQL support

## Roadmap to 1.0

Vantage needs a bit more work. Large number of features is already implemented, but
some notable features are still missing:

- Vantage need Real-World app implementation for a backend as a test-case. I had some
  issues with UUID fields, there could have been some other issues too.
- Vantage works with PostgreSQL but not with GraphQL. I'll need to implement them both
  to make Vantage more usable for the frontend applications.
- Implement better ways to manipulate conditions, fields etc. If we can add those
  dynamically, we should also be able to remove them too.
- Vantage supports only base types (subtypes of serde_json::Value). I'll need to implement
  additional DataSource-specific columns.
- We need DataSource support for a regular REST APIs and implement example of Vantage
  used as WASM interface between React components and the backend server.
- I'd like to create example for Sycamore and open-source Tailwind components, showing
  how multiple independent components can interact through signals and manipulate
  a dataset collectively, fetching data when needed.
- An example for Egui would also be nice.
- I have Associated queries already implemented, but I also want to have associated entities,
  which can have their types manipulated, validated and saved back. Associated entity should
  work with dynamic forms.

## Installation

Just type: `cargo add vantage`

If you like what you see so far - reach out to me on BlueSky: [nearly.guru](https://bsky.app/profile/nearly.guru)

# Walkthrough

(You can run this [example](bakery_model/examples/0-intro.rs) with `cargo run --example 0-intro`)

Vantage interract with your data through a unique concept called "Data Sets". Your application will
work with different sets suc has "Set of Clients", "Set of Orders" and "Set of Products" etc.

It's easier to explain with example. Your SQL table "clients" contains multiple client records. We
do not know if there are 10 or 9,100,000 rows in this table. We simply refer to them as "set of
clients".

Vantage defines "Set of Clients" is a Rust type, such as `Table<Postgres, Client>`:

```rust
let set_of_clients = Client::table();   // Table<Postgres, Client>
```

Any set can be iterated over, but fetching data is an async operation:

```rust
for client in set_of_clients.get().await? {   // client: Client
    println!("id: {}, client: {}", client.id, client.name);
}
```

In a production applications you wouldn't be able to iterate over all the records like this,
simply because of the large number of records. Which is why we need to narrow down our
set_of_clients by applying a condition:

```rust
let condition = set_of_clients.is_paying_client().eq(&true);  // condition: Condition
let paying_clients = set_of_clients.with_condition(condition);  // paying_clients: Table<Postgres, Client>
```

If our DataSource supports record counting (and SQL does), we can simply fetch through count():

```rust
println!(
    "Count of paying clients: {}",
    paying_clients.count().get_one_untyped().await?
);
```

Now that you have some idea of what a DataSet is, lets look at how we can reference
related sets. Traditionally we could say "one client has many orders". In Vantage we say
"clients set refers to orders set":

```rust
let orders = paying_clients.ref_orders();   // orders: Table<Postgres, Order>
```

Type is automatically inferred, I do not need to specify it. This allows me to define
a custom method on Table<Postgres, Order> inside `bakery_model` and use it anywhere:

```rust
let report = orders.generate_report().await?;
println!("Report:\n{}", report);
```

Importantly - my implementation for `generate_report` comes with a unit-test. Postgres
is too slow for unit-tests, so I use a mock data source. This allows me to significantly
speed up my business logic test-suite.

One thing that sets Vantage apart from other ORMs is that we are super-clever at building
queries. `bakery_model` uses a default entity type Order but I can supply another struct type:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct MiniOrder {
    id: i64,
    client_id: i64,
}
impl Entity for MiniOrder {}
```

`impl Entity` is needed to load and store "MiniOrder" in any Vantage Data Set. Next I'll use
`get_some_as` which gets just a single record from set. The scary-looking method
`get_select_query_for_struct` is just to grab and display the query to you:

```rust
let Some(mini_order) = orders.get_some_as::<MiniOrder>().await? else {
    panic!("No order found");
};
println!("data = {:?}", &mini_order);
println!(
    "MiniOrder query: {}",
    orders
        .get_select_query_for_struct(MiniOrder::default())
        .preview()
);
```

Vantage adjusts query based on fields defined in your struct. My `MegaOrder` will remove `client_id` and
add `order_total` and `client_name` instead:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct MegaOrder {
    id: i64,
    client_name: String,
    total: i64,
}
impl Entity for MegaOrder {}

let Some(mini_order) = orders.get_some_as::<MegaOrder>().await? else {
    panic!("No order found");
};
println!("data = {:?}", &mini_order);
println!(
    "MegaOrder query: {}",
    orders
        .get_select_query_for_struct(MegaOrder::default())
        .preview()
);
```

If you haven't already, now is a good time to run this code. Clone this repository and run:

```bash
$ cargo run --example 0-intro
```

At the end, example will print out both queries. Lets dive into them:

```sql
SELECT id, client_id
FROM ord
WHERE client_id IN (SELECT id FROM client WHERE is_paying_client = true)
  AND is_deleted = false;
```

`MiniOrder` only needed two fields, so only two fields were queried.

Condition on "is_paying_client" is something we implicitly defined when we referenced Orders from
`paying_clients` Data Set. Wait. Why is `is_deleted` here?

As it turns out - our table definition is using extension `SoftDelete`. In the `src/order.rs`:

```rust
table.with_extension(SoftDelete::new("is_deleted"));
```

This extension modifies all queries for the table and will mark records as deleted when you
execute table.delete().

The second query is even more interesting:

```sql
SELECT id,
    (SELECT name FROM client WHERE client.id = ord.client_id) AS client_name,
    (SELECT SUM((SELECT price FROM product WHERE id = product_id) * quantity)
    FROM order_line WHERE order_line.order_id = ord.id) AS total
FROM ord
WHERE client_id IN (SELECT id FROM client WHERE is_paying_client = true)
  AND is_deleted = false;
```

As it turns out - there is no physical field for `client_name`. Instead Vantage sub-queries
`client` table to get the name. The implementation is, once again, inside `src/order.rs` file:

```rust
table
  .with_one("client", "client_id", || Box::new(Client::table()))
  .with_imported_fields("client", &["name"])
```

The final field - `total` is even more interesting - it gathers information from
`order_line` that holds quantities and `product` that holds prices.

Was there a chunk of SQL hidden somewhere? NO, It's all Vantage's query building magic. Look inside
`src/order.rs` to see how it is implemented:

```rust
table
  .with_many("line_items", "order_id", || Box::new(LineItem::table()))
  .with_expression("total", |t| {
    let item = t.sub_line_items();
    item.sum(item.total()).render_chunk()
  })
```

Where is multiplication? Apparently item.total() is responsible for that, we can see that in
`src/lineitem.rs`.

```rust
table
  .with_one("product", "product_id", || Box::new(Product::table()))
  .with_expression("total", |t: &Table<Postgres, LineItem>| {
    t.price().render_chunk().mul(t.quantity())
  })
  .with_expression("price", |t| {
    let product = t.get_subquery_as::<Product>("product").unwrap();
    product.field_query(product.price()).render_chunk()
  })
```

### Conclusion

We have discovered that behind a developer-friendly and very Rust-intuitive Data Set
interface, Vantage offers some really powerful features and hides complexity.

What does that mean to your developer team?

You would need to define business entities once, but the rest of your team/code can focus on the
business logic - like improving that `generate_report` method!

My example illustrated how Vantage provides separation of concerns and abstraction of complexity - two
very crucial concepts for business software developers.

Use Vantage. No tradeoffs. Productive team! Happy days!

### Components of Vantage

To understand Vantage in-depth, you would need to dissect and dig into its individual components:

1. DataSet - like a Map, but Rows are stored remotely and only fetched when needed.
2. Expressions - recursive template engine for building SQL.
3. Query - a dynamic object representing a single SQL query.
4. DataSources - an implementation trait for persistence layer. Can be Postgres, a mock (more implementations coming soon).
5. Table - DataSet with consistent columns, condition, joins and other features of SQL table.
6. Field - representing columns or arbitrary expressions in a Table.
7. Busines Entity - a record for a specific DataSet (or Table), such as Product, Order or Client.
8. CRUD operations - insert, update and delete records in DataSet through hydration.
9. Reference - ability for DataSet to return related DataSet (get client emails with active orders for unavailable stock items)
10. Joins - combining two Tables into a single Table without hydration.
11. Associated expression - Expression for specific DataSource created by operation on DataSet (sum of all unpaid invoices)
12. Subqueries - Field for a Table represented through Associated expression on a Referenced DataSet.
13. Aggregation - Creating new table from subqueries over some other DataSet.
14. Associated record - Business Entity for a specific DataSet, that can be modified and saved back.

A deep-dive into all of those concepts and why they are important for business software developers
can be found in the [Vantage Book](https://romaninsh.github.io/vantage).

## Current status

Vantage currently is in development. See [TODO](TODO.md) for the current status.

## Author

Vantage is implemented by **Romans Malinovskis**. To get in touch:

- <https://www.linkedin.com/in/romansmalinovskis>
- <https://bsky.app/profile/nearly.guru>
