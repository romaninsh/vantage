# DORM

[![Book](https://github.com/romaninsh/dorm/actions/workflows/book.yaml/badge.svg)](https://romaninsh.github.io/dorm/)

DORM is a type-safe, ergonomic database toolkit for Rust that focuses on developer productivity
without compromising performance. It allows you to work with your database using Rust's strong type
system while abstracting away the complexity of SQL queries. (Support for NoSQL databases is coming soon)

## Quick Start

Your application would typically require a model definition. Here is example:
[bakery_model](bakery_model/src/). You would also need a Postgres database populated with sample data
from [schema-pg.sql](bakery_model/schema-pg.sql) and create role `postgres`.

Once this is in place, you can use DORM to interract with your data like this:

```rust
use dorm::prelude::*;
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

SQL generated by DORM and executed:

```sql
SELECT id,
    (SELECT name FROM client WHERE client.id = ord.client_id) AS client_name,
    (SELECT SUM((SELECT price FROM product WHERE id = product_id) * quantity)
    FROM order_line WHERE order_line.order_id = ord.id) AS total
FROM ord
WHERE client_id IN (SELECT id FROM client WHERE is_paying_client = true)
  AND is_deleted = false;
```

This illustrates how DORM combined specific rules of your code such as "only paying clients" with
the rules defined in the [bakery_model](bakery_model/src/), like "soft-delete enabled for Orders" and "prices are
actually stored in product table" and "order has multiple line items" to generate a single
and efficient SQL query.

## Key Features

- 🦀 **Rust-first Design** - Leverages Rust's type system for your business entities
- 🥰 **Complexity Abstraction** - Hide complexity away from your business logic
- 🚀 **High Performance** - Generates optimal SQL queries
- 🔧 **Zero Boilerplate** - No code generation or macro magic required
- 🧪 **Testing Ready** - First-class support for mocking and unit-testing
- 🔄 **Relationship Handling** - Elegant handling of table relationships and joins
- 📦 **Extensible** - Easy to add custom functionality and non-SQL support

## Installation

DORM is still in development. It is not in crates.io yet, so to install it you will need to clone
this repository and link it to your project manually.

If you like what you see so far - reach out to me on BlueSky: [nearly.guru](https://bsky.app/profile/nearly.guru)

## Introduction

(You can run this [example](bakery_model/examples/0-intro.rs) with `cargo run --example 0-intro`)

DORM interract with your data through a unique concept called "Data Sets". Your application will
work with different sets suc has "Set of Clients", "Set of Orders" and "Set of Products" etc.

It's easier to explain with example. Your SQL table "clients" contains multiple client records. We
do not know if there are 10 or 9,100,000 rows in this table. We simply refer to them as "set of
clients".

DORM defines "Set of Clients" is a Rust type, such as `Table<Postgres, Client>`:

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
related sets. Traditionally we could say "one client has many orders". In DORM we say
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

One thing that sets DORM apart from other ORMs is that we are super-clever at building
queries. `bakery_model` uses a default entity type Order but I can supply another struct type:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct MiniOrder {
    id: i64,
    client_id: i64,
}
impl Entity for MiniOrder {}
```

`impl Entity` is needed to load and store "MiniOrder" in any DORM Data Set. Next I'll use
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

DORM adjusts query based on fields defined in your struct. My `MegaOrder` will remove `client_id` and
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

As it turns out - there is no physical field for `client_name`. Instead DORM sub-queries
`client` table to get the name. The implementation is, once again, inside `src/order.rs` file:

```rust
table
  .with_one("client", "client_id", || Box::new(Client::table()))
  .with_imported_fields("client", &["name"])
```

The final field - `total` is even more interesting - it gathers information from
`order_line` that holds quantities and `product` that holds prices.

Was there a chunk of SQL hidden somewhere? NO, It's all DORM's query building magic. Look inside
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
interface, DORM offers some really powerful features and hides complexity.

What does that mean to your developer team?

You would need to define business entities once, but the rest of your team/code can focus on the
business logic - like improving that `generate_report` method!

My example illustrated how DORM provides separation of concerns and abstraction of complexity - two
very crucial concepts for business software developers.

Use DORM. No tradeoffs. Productive team! Happy days!

### Components of DORM

To understand DORM in-depth, you would need to dissect and dig into its individual components:

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
can be found in the [DORM Book](https://romaninsh.github.io/dorm).

## Current status

DORM currently is in development. See [TODO](TODO.md) for the current status.

## Author

DORM is implemented by **Romans Malinovskis**. To get in touch:

- <https://www.linkedin.com/in/romansmalinovskis>
- <https://bsky.app/profile/nearly.guru>
