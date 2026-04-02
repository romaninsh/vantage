# Fetching Data

So far we were creating tables and were generating queries
from them, but wouldn't it be great if we could fetch data
directly from the table object?

If you recall - our table is already associated with DataSource,
but when we execute `get_select_query()` the resulting Query
is not associated with any DataSource.

There is a type called `AssociatedQuery` that can be produced
by a table object and it has some beautiful methods to
actually fetch data.

Lets explore:

```rust
let low_cal_products = model::LowCalProduct::table();

let count = low_cal_products.count().get_one().await?;
```

Table::count() is a method that returns a query for counting
rows in a table. Because our LowCalProducts contains a
condition, this will return a count of rows that match
the condition.

You could use count().preview() still to confirm,
that Vantage won't attept to fetch all records and iterate
over them, but instead will use a SUM() function to count rows:

```sql
SELECT COUNT(*) FROM "product" WHERE "calories" <= 100
```

Calling `get_one` will instead allow us to fetch the value
directly into a variable. We do not know what type our query
would produce, so get_one() returns json::Value and you can
use "as_i64" to cast it into a numeric type:

```rust
let count = model::LowCalProduct::table()
    .count()
    .get_one()
    .await?
    .as_i64()?
```

Previously I was using unwrap() and now I am using "?" to unwrap.
This is because previously our code was certain that a field
woudl exist, since we added it ourselves, but with .get_one()
we are not sure about the response. Perhaps query execution
would fail, so we need a proper error handling.

Lets explore another method:

```rust
let total_calories = model::LowCalProduct::new()
    .sum(model::Product::calories())
    .get_one()
    .await?
    .as_i64()?
```

Here we are passing a Column object to the sum() method.

Some of those methods will be useful for us later, but for now
lets look at the way to fetch data from a table:

```rust
for product in model::LowCalProduct::table().get_all_data().await? {
    println!("{} ({} kcal)", product["name"]?, product["calories"]?);
}
```

Next we will look at how to use power of Deserialize trait
of your custom type.
