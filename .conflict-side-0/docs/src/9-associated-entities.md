# Associated Entities

Do you remember how we used Query type only to discover that
there is also AssociatedQuery type?

Well, in the last chapter we used our custom entity type
Product and now it turns out you can have an associated
entity type too.

AssociatedEntity<T> is the way how your entity can remain
linked to the Table object. To save you from headache of
object lifetimes, it will actually contain a clone of a
table as well as ID field. Therefore your type will
no longer need an "id" field.

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct Product {
    name: String,
    calories: i64,
}
```

Now when we deal with associated entities, we load() and save()
them:

```rust
let product = model::Product::table().load<Product>(4).await?;

product.calories = 56;
product.save().await?;
```

AssociatedEntity derefs itself so that you can still access
the fields. Additionally the following methods are added:

- reload() - reload entity from the database
- id() - return id of the entity
- delete() - delete the entity from the database
- save() - saves the entity to the database
- save_into() - saves entity into a different table

Here is example:

```rust
if let Some(product) = model::LowCalProduct::table().load_any::<Product>().await? {
    writeln!("Low-cal Product {} has {} calories", product.name, product.calories);
    product.calories += 100; // no londer LowCalProduct

    // product.save().await?; // will Err because of condition
    product.save_into(model::Product::table()).await?;
}
```

It should no longer be a surprise to you that you can do the exactly same
stuff with a table which relies on Join:

```rust

struct ProductInventory {
    name: String,
    stock: i64,
}

if let Some(product) = model::Product::with_inventory().load::<ProductInventory>(4).await? {
    writeln!("Product {} has {} items in stock", product.name, product.stock);
    if product.stock > 0 {
        product.stock -= 1;
        product.save().await?;
    } else {
        product.delete().await?;
    }
}
```

Vantage will automatically understand, which fields you have changed (stock)
and will only update those fields. Vantage will also delete the row from
both "product" and "inventory" tables if stock is 0.

Your code remains intuitive, while Vantage takes care of the rest, but
lets make the code even better:

```rust

impl AssociatedEntity<ProductInventory> {
    pub async fn sell(self, qty: i64) -> Result<()> {
        if qty > self.stock {
            return Err(anyhow::anyhow!("Not enough items in stock"));
        }
        self.stock -= qty;
        if self.stock == 0 {
            self.delete().await?;
        } else {
            self.save().await?;
        }
        Ok(())
    }
}
```

Now you can use your method like this:

```rust
let product = model::Product::with_inventory().load::<ProductInventory>(4).await?;
product.sell(10).await?;
```

Rust never looked so good!

But hey, that's not all. Vantage also supports associations between two tables.
Keep reading!
