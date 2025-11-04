# Joins

Join can happen between two tables with one-to-some(one) relationship.

A good example for us is if we add a new table called `inventory` that
joins `product` table:

```sql
CREATE TABLE inventory (
    product_id SERIAL PRIMARY KEY,
    stock INT DEFAULT NULL
);
```

In this case, inventory does not particularly useful ol its own,
so we can make it part of the Products type:

```rust
impl Product {
    pub fn table_with_inventory() -> Self {
        Product::table()
            .with_alias("p")
            .with_join(
                Table::new("inventory", postgres())
                    .with_alias("i")
                    .with_id_field("product_id")
                    .with_field("stock"),
                "id",
            )
        }
    }
}
```

The beautiful syntax here is going to give you exactly
what you expect:

```rust
let prod_inv = model::Product::table_with_inventory();
writeln!(prod_inv.get_select_query().preview());

// renders into: SELECT p.id, p.name, i.stock FROM product p LEFT JOIN inventory i ON (p.id = i.product_id)
```

How is that possible? Well, Vantage's "with_join" method will
consume the table object that you pass and will move its
fields into the original table. The joined table will be
wrapped into a Join object and will instruct query builder
to add join into your query.

There are ways to create different kind of joins too, but
api for that is not yet stable.

As you will see later, Vantage operates with joined tables
just as good as with a single table.

Now is probably a great time to look into Vantages capabilities
of operating with your entities.

So far we only used Table to create select queries, but
we can in fact hydrate our entities with data from the
database.
