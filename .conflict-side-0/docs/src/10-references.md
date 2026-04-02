# References

Vantage allows you to connect tables together. Lets create two new tables
in addition to "Product" that we have already:

```rust
struct Order {
    product: Product,
    client: Client,
}

struct Client {
    name: String,
    contact_details: String,
}
```

The definition of the tables would be just like in chapter 5-Entity Model, however
we will use "has_one" and "has_many" methods:

```rust
impl Product {
    pub fn static_table() -> &'static Table<Postgres> {
        static TABLE: OnceLock<Table<Postgres>> = OnceLock::new();

        TABLE.get_or_init(|| {
            Table::new("product", postgres())
                .with_id_field("id")
                .with_field("name")
                .has_many("orders", "product_id", || Order::table())
        })
    }
}


impl Order {
    pub fn static_table() -> &'static Table<Postgres> {
        static TABLE: OnceLock<Table<Postgres>> = OnceLock::new();

        TABLE.get_or_init(|| {
            Table::new("order", postgres())
                .with_id_field("id")
                .with_field("name")
                .has_one("client", "client_id", || Client::table())
                .has_one("product", "product_id", || Product::table())
        })
    }
}

impl Client {
    pub fn static_table() -> &'static Table<Postgres> {
        static TABLE: OnceLock<Table<Postgres>> = OnceLock::new();

        TABLE.get_or_init(|| {
            Table::new("client", postgres())
                .with_id_field("id")
                .with_field("name")
                .with_field("contact_details")
                .has_many("orders", "client_id", || Order::table())
        })
    }
}
```

Given one Table, Vantage lets you traverse relationships between tables.
Lets say we want to see how many orders does product with id=4 have:

```rust
let product = model::Product::table().with_id(4);

let orders_count = product.get_ref("orders").unwrap().count().get_one().await?;
```

Here is what happens under the hood:

1.  A query is generated for the Product where id=4
2.  product query is used as a condition for a order query
3.  order query is executed for counting rows

```sql
SELECT COUNT(*) FROM order WHERE (product_id = in (SELECT id FROM product WHERE (id = 4)));
```

This query may seem a bit redundant, but lets see how many LowCalProduct
orders we have:

```rust
let product = model::LowCalProduct::table();

let low_cal_orders = product.get_ref("orders").unwrap().count().get_one().await?;
```

Resulting query now looks like this:

```sql
SELECT COUNT(*) FROM order WHERE (product_id IN (SELECT id FROM product WHERE (calories < 100)));
```

In Vantage relationship traversal is always converting one "set" into another "set".

In fact - if you want to calculate how many clients have placed orders for
Low Calory Products, you can do it like this:

```rust
let low_cal_products = model::LowCalProduct::table();
let clients_on_diet = low_cal_products
    .get_ref("orders")
    .unwrap()
    .get_ref("client")
    .unwrap()
    .count()
    .get_one()
    .await?;
```

But lets not stop here. Suppose you wanted to send all clients who are
on a diet some email about a new product:

```rust
let low_cal_products = model::LowCalProduct::table();
let clients_on_diet = low_cal_products
    .get_ref("orders")
    .unwrap()
    .get_ref("client")
    .unwrap();

for client in clients_on_diet.get<Client>().await? {
    client.send_email("New low carb product is out!").await?;
}
```

Imagine all the other things you could do. Yet once again,
Vantage has more surprises for you.

We have learned about expressions before, right? Well, expressions
can be built from subqueries.
