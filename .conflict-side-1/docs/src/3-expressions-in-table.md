# Expressions in a Table

We now know, that Query can accept expressions pretty much anywhere, but
what about `Table`?

## `add_expression()` and `with_expression()`

Table treats expressions lazily. Unless the Entity you are using
has a field that matches expression field name, it will not be evaluated.

To define expression use:

- `add_expression` - define a callback returning Expression for a field
- `with_expression` - just like `add_expression` but returns modifield Self

Lets define a field that returns current time:

```rust
table.with_expression("current_time", || expr!("now()"));
```

In our introduction example, we came across a field: `total`:

```rust
// lineitem.rs
table.with_expression("total", |t: &Table<Postgres, LineItem>| {
    t.price().render_chunk().mul(t.quantity())
})
```

## Chunks and Expressions

Any `Chunk` can be converted into `Expression` but executing `render_chunk()`. If
you call `render_chunk()` on a Query, it will flattern itself into a static `Expression` type.

a callback that `with_expression` accepts, is expected to return `Expression`, but you
can use various ways on how to build it.

For instance, we can get a query from related table:

```rust
.with_many("line_items", "order_id", || Box::new(LineItem::table()))
.with_expression("total", |t| {
    let item = t.sub_line_items();
    item.sum(item.total()).render_chunk()
})

// you also need this:
pub trait OrderTable: SqlTable {}
impl OrderTable for Table<Postgres, Order> {
    fn sub_line_items(&self) -> Table<Postgres, LineItem> {
        self.get_subquery_as("line_items").unwrap()
    }
}
```

Relationship between entities is defined by `with_many` and `with_one` methods.
Traversing this relationship can be done by `get_ref()` and `get_subquery()` methods.

We have already explored `get_ref()` earlier, but how is `get_subquery()` different?

- `get_ref()` - given a DataSet - return related DataSet: `SELECT * FROM line_items WHERE order_id IN (SELECT id FROM ord)`
- `get_subquery()` - Will return Table with a condition linking to the current table: `SELECT * FROM line_items WHERE order_id = ord.id`

`get_subquery()` only makes sense if you make it part of the `Order` table query. It's perfect for
us to aggregate sum of `total`s:

```rust
let item = t.sub_line_items();
item.sum(item.total()).render_chunk()
```

Here `item` is of type `Table<Postgres, LineItem>` which means we can use `Table::sum()` and custom
method `Table<Postgres, LineItem>::total()`.

Method `sum` returns `Query` (impl `Chunk`) which we can convert into `Expression` using `render_chunk()`.

BTW - `sum` accepts `Chunk` as an argument, and in our case that is just fine, because `total()` returns
a chunk and not a Column.

## Another Example

As our last example, lets look at LineItem implementation of a `price` field. This filed is implemented
through expression again, as we don't have a physical column fro it:

```rust
.with_one("client", "client_id", || Box::new(Client::table()))
.with_expression("price", |t| {
    let product = t.get_subquery_as::<Product>("product").unwrap();
    product.field_query(product.price()).render_chunk()
})
```

I haven't defined method like `sub_line_items` before, so I'm using `get_subquery_as` directly.
There is also `get_subquery` but it returns Box<dyn SqlTable>. I want to use `product.price()` so
instead I'm using `get_subquery_as` and specifying entity type explicitly.

As far as Vantage is concerned, it sees `with_one` and `with_many` relationships equally. I need to
think about this though. If subquery could return multiple rows, I'd need to have them limited,
aggregated or wrapped into a string somehow (like using ExpressionArc::fx())

In this case I don't have to worry about that. I just need to query a single field (that happens to
be a column `price`).

## Expressions in Conditions

Previously I have used the following code:

```rust
let set_of_clients = Client::table();

let condition = set_of_clients.is_paying_client().eq(&true);
let paying_clients = set_of_clients.with_condition(condition);
```

Condition is a struct, that implements `Chunk` trait. You can probably guess that `Table`
will apply those `Condition`s into resulting Queries and that's it.

Condition consists of `field` , `operation` and `value`:

- `field`: Can be a `Column`, `Expression`, another `Condition` or a `serde_json::Value`
- `operation` is just a string
- `value` is a `Chunk`

The reason why we don't simply throw everything into `Expression` - our `Column` might need
to use an alias (and sometimes we change alias as we join tables)

## Operations

`Operations` is another Trait that allows type to participate in SQL operations. Both `Column` and `Expression` and `Value`
implement `Operations`, this allows this:

```rust
let condition = column.eq(&"John")

// or

let condition = expr!("now()").lt(&yesterday)
```

Conditions created by the operation can be used in `with_condition()` method.

## Conclusion

In Vantage - expressions and operations create a powerful way to extend of high-level `Table` features
such as conditions and expression fields.

Functionality is composable of underlying components. If `Table` does not implement a feature you
like, you can easily build it using Expressions and Operations.
