# Data Sets

Traditional ORMs operate with records. If you have used SeaORM or Diesel - you need
to temporarily "un-learn" those. Vantage syntax may look similar, but it's very different to
the classic ORM pattern:

1. **Vantage operates with Data Sets**. A set can contain either a single record, no records or
   a huge number of records. Set represents records in remote database (or API) and does
   not store anything in memory.

2. **Vantage executes operations remotely**. Changing multiple records in ORM involves
   fetching all the records, modifying them and saving them back. Vantage prefers to
   execute changes remotely, if underlying DataSource supports it.

As a developer, you will always know when Vantage interacts with the database, because
those operations are async. Majority of Vantage operations - like traversing relationship,
calculating sub-queries - those are sync.

## `sql::Table` and `sql::Query`

Vantage implements sql::Table and sql::Query - because we all love SQL. However you can
define other data sources - such as NoSQL, REST API, GraphQL, etc. Those extensions
do not need to be in same crate as Vantage. For the rest of this book I will only
focus on SQL as a predominant use-case.

Vantage is quite fluid in the way how you use `table` and `query`, you can use one to
compliment or create another, but they serve different purpose:

1. `sql::Table` has a structure - fields, joins and relations are defined dynamically.
2. `sql::Query` on other hand is transient. It consists of smaller pieces which we call `sql::Chunk`.
3. `sql::Table` can create and return `sql::Query` object from various methods

`sql::Chunk` trait that is implemented by `sql::Query` (or other arbitrary expressions)
acts as a **glue** betwene tables. For instance when you run traverse relations:

```rust
let client = Client::table().with_id(1);

let order_lines = client.ref_orders().ref_order_lines();
```

1. Vantage executes `field query` operation on a client set for field `id`.
2. Vantage creates `orders` table and adds `condition` on `client_id` field.
3. Vantage executes `field query` operation on `orders` table for field `id`.
4. Vantage creates `order_lines` table and adds `condition` on `order_id` field.

Vantage weaves between `sql::Table` and `sql::Query` without reaching out to the
database behind a simple and intuitive code.

## ReadableDataSet and WritableDataSet

Vantage provides two traits: `ReadableDataSet` and `WritableDataSet`. As name
suggests - you can fetch records from Readable set. You can add, modify or delete
records in Writable set.

Vantage implements those traits:

- `sql::Table` - implements both `ReadableDataSet` and `WritableDataSet`.
- `sql::Query` - implements `ReadableDataSet` only.

## Operating with Data sets

At the very basic level - you can iterate through a readable data set.

```rust
let set_of_clients = Client::table();

for client in set_of_clients.get().await? {
    println!("{}", client.name);
}
```

There are more ways to fetch data from `ReadableDataSet`:

- `get` - returns all records in a Vec using default entity type
- `get_as` - return all records using a custom type
- `get_all_untyped` - return all records as a raw JSON object
- `get_some` and `get-some_as` - return only one record (or none)
- `get_row_untyped` - return single record as a raw JSON object
- `get_col_untyped` - return only a single column as a raw JSON values
- `get_one_untyped` - return first column of a first row as a raw JSON value

In most cases you would use `get` and `get_some`:

```rust
let client = Client::table().with_id(1);

let Some(client_data) = client.get_some().await? else { // fetch single record
    // handle error
};

for client_order in client.orders().get().await? { // fetch multiple records
    println!("{}", client_order.id);
}
```

## Creating Queries from Tables

Sometimes you do not want result, but would prefer a query object instead. This gives you
a chance to tweak a query or use it elsewhere.

Vantage provides trait TableWithQueries that generates Query objects for you:

- `get_empty_query` - returns a query with conditions and joins, but no fields
- `get_select_query` - like `get_empty_query` but adds all physical fields
- `get_select_query_for_field_names` - Provided with a slice of field names and expressions, only includes those into a query.
- `get_select_query_for_field` - Provided a query for individual field or
  expression, which you have to pass through an argument.
- `get_select_query_for_fields` - Provided a query for multiple fields

There are generally two things you can do with a query:

1. Tweak and execute it
2. Use it as a `Chunk` elsewhere

### Tweaking and Executing a query

```rust
let vip_orders = Client::table().add_condition(Client::table().is_vip().eq(true)).ref_orders();

let query = vip_orders
    .get_select_query_for_field_names(&["id", "client_id", "client"]) // excluding `total` here
    .with_column("total".to_string(), expr!("sum({})", vip_orders.total())) // add as aggregate
    .with_column("order_count".to_string(), expr!("count(*)"))
    .with_group_by(vip_orders.client_id());

let result = postgres().query_raw(&query).await?;
```

### Using a query as a `Chunk` elsewhere

```rust
// TODO - hypothetical example, not implemented in bakery_model

let product_123 = Product::table().with_code("PRD-123");
let john = Client::table().with_email("john@example.com");

let new_order = Order::table()
    .insert(Order {
        product: product_123,
        client: john,
        quantity: 1,
    }).await?;
```

```rust
// TODO: test
let john = Client::table().with_email("john@example.com");

let order = Order::table()
    .with_condition(Order::table().client_id().in(john.get_select_query_for_field(john.id())))
```

Method `get_ref()` does exactly that, when you traverse relationships.

## Conclusion

DataSet is a powerful concept that sets aside Vantage from the usual ORM pattern.
`sql::Table` and `sql::Query` are the building blocks you interract with most
often in Vantage.

Understanding this would allow you to implement missing features (such as table grouping)
and eventually devleop extensions for your data model.
