# Struct Entities

So far we have defined our Product type without any fields.
Lets add some fields to it:

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct Product {
    id: Option<i64>,
    name: String,
    calories: i64,
}
```

You can now use get<Product>() to fetch data from the database
much more conveniently:

```rust
for product in model::LowCalProduct::table().get<Product>().await? {
    println!("{} ({} kcal)", product.name, product.calories);
}
```

There is now your type safety. You also can see that we can
use any type that implements Serialize and Deserialize traits
in get() method.

However, now that we have implemented a struct for our entity,
we can start modifying our data. Check this code out:

```rust
model::LowCalProduct::table().map<Product>(|product| async move {
    product.calorues += 1;
})().await?;
```

This allows you to iterate over your data as you did before,
but map() will also store data back to the database. You
just need to remember to have `id` field in your struct. Here
is what happens:

1.  Vantage determines the type of your struct (Product)
2.  Vantage will fetch row from a query that includes your condition
3.  Vantage will deserialize the row into your struct
4.  Vantage will call your closure
5.  Vantage will serialize your struct back to a row
6.  Vantage will replace row in the database with the new values

The map() method does not know if you have changed struct, so
it will always try to execute "REPLACE" query and based on
your unique id field, it should rewrite the row.

You can also use insert() method on your table to add a new
row to the database:

```rust
model::Product::table().insert(Product {
    id: None,
    name: "New Product".to_string(),
    calories: 100,
})
.await?;
```

Deleting is also possible, but you need to be careful. delete_all()
method will remove all rows.

```rust
model::LowCalProduct::table().delete_all().await?;
```

If you want to delete a specific row, you can set a condition:

```rust
model::Product::table().with_id(1).delete_all();
```

Although a more convenient method delete() exists too:

```rust
model::Product::table().delete(1).await?;
```

I should probably mention, that delete() method will not
affect rows in a table, which do not match the condition:

```rust
model::LowCalProduct::table().delete(4).await?;
```

Here a row with id=4 will only be deleted if calories is
less than 100 for this row.

This will be useful for us later.

Next I want to tell you about associated entities.
