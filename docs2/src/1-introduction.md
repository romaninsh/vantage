# Introduction

````admonish tip title="TL;DR"

Vantage is a Rust framework for
 - operating on remote DataSet reflections in real-time;
 - converting intent/expressions into sophisticated SQL queries;
 - offering augmented entities to generic components;

The core concepts enable transparent implementation for complex entity features such as:
 - transformation: references, aggregation, joins, expressions, conditions, column mapping and containtment;
 - behaviour: soft-delete, validation, hooks, default values;
 - source abstraction: SQL, CSV, APIs, NoSQL

```rust
// A remote set of VIP Clients
let vip_clients = Client::table()
    .with_condition(Client::is_vip().eq(true));

// A remote set of Overdue Invoices for VIP Clients
let overdue_invoices = vip_clients.ref_invoices()
    .my_overdue_filter();

// Use generic function to build query, fetch data and construct table headers
render_any_table_with_headers(Box::new(overdue_invoices)).await()?;
```
````

## Problem Definition

Rust works really well with local data:

```rust
struct Line {
    id: i32,
    quantity: i32,
    price: i32,
}
struct Invoice {
    id: i32,
    lines: Vec<Line>,
    due_date: Date,
}
struct Client {
    id: i32,
    name: String,
    invoices: Vec<Invoice>,
    is_vip: bool,
}
let clients = load_clients_local();
let due_total = clients.iter().map(|client| {
    client.invoices.iter().map(|invoice| {
        invoice.lines.iter().map(|line| {
            line.price * line.quantity
        }).sum()
    }).sum()
}).sum();
```

However if data is not stored locally and if you have significant volume of records, Rust can no
longer rely on it's type system.

One option is to write custom SQL queries:

```sql
SELECT SUM(price*quantity) FROM lines WHERE invoice_id in (
    SELECT id FROM invoices WHERE client_id in (
        SELECT id FROM clients WHERE is_vip = true
    )
);
```

Rust types are powerless with custom SQL queries. Other option is to use ORM, but it would be very
slow as most ORMs cannot execute SQL efficiently.

## How Vantage addresses these challenges

Vantage implements a concept of DataSet<Entity>. Think of it as Vec<Client>, but records are stored
in a remote database. DataSets are **lazy** and **composable**. Our code does not know how many
records match our criteria, but can still operate on that abstract set:

```rust
let clients = Client::table();
let due_total = clients.ref_orders().ref_lines().sum(Lines::total()).await?;
```

Only when we call `await` the query will be built and executed. Logically speaking,
we needed to combine **DataSet** with **Intent** to generate a query and execute it
producing a **Result**:

```kroki-plantuml
@startmindmap
skinparam monochrome true
+ Query
-- DataSet(Lines)
--- DataSet(Orders)
---- DataSet(Clients)
-- Intent(sum)
++ Result
@endmindmap
```

```admonish tip title="Summary"

**Vantage** operates with DataSets and Intents. You can manipulate DataSets inexpensively and when
you are ready - intent will allow you to fetch the data.

```
