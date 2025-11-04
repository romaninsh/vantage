# Subquery Expressions

Once we have used "has_one" and "has_many" methods, we can now
add some expressions as well.

I'll start by extending our "Client::table()" with "total_orders" field.
Place that inside static_table() method:

```rust
Table::new("client", postgres())
    .with_id_field("id")
    .with_field("name")
    .with_field("contact_details")
    .has_many("orders", "client_id", || Order::table())
    .with_expression("total_orders", move |t| {
        t.get_ref_related("orders").unwrap().count()
    })
```

What happens here then?

1.  A query is generated for the Client
2.  If "total_orders" field is requested, a callback is called
3.  The callback is passed a Table object ("client"), which has a reference to the "orders" table
4.  get_ref_related() is similar to get_ref(), but is suitable for subquery expressions
5.  get_ref_related() returns a Table object
6.  count() is called on the Table object, producting a SqlChunk object
7.  The SqlChunk object is is aliased into "total_orders" field inside Query
8.  Query is executed

Lets also modify "Client" struct:

```rust
struct Client {
    name: String,
    contact_details: String,
    total_orders: Option<i64>
}
```

Now that we have know how many orders clients have placed, we can use it
as a condition.

```rust
let vip_clients = clients.with_condition(clients.field("total_orders").gt(4));
```

Lets also calculate how many low_cal_orders clients have placed:

```rust
.with_expression("low_cal_orders", move |t| {
    t
        .get_ref_related("orders")
        .with_condition(t.get_filed("calories").unwrap().lt(100))
        .unwrap()
        .count()
})
.with_expression("high_cal_orders", move |t| {
    t
        .get_ref_related("orders")
        .with_condition(t
            .get_field("calories")
            .unwrap()
            .gt(100)
            .or(t.get_filed("calories").unwrap().eq(100))
        )
        .unwrap()
        .count()
})
```

We just casually added 2 more expressions to our Client table. Those
normally won't be queried unless needed. However, we can use them
to calculate conditions:

```rust
let diet_clients = clients
    .with_condition(clients
        .field("low_cal_orders")
        .gt(clients.field("high_cal_orders"))
    );
```

Finally to clean things up a bit, we can move some of this logic
into our `model/*.rs` files.

Overall, you are now familiar with the basics of Vantage and can start
building business model abstractions for your application.
