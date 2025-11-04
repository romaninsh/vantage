# Table Columns

`Table` allows you to define and reference columns. A `Column` is another struct in Vantage.
Using columns outside of table context is convenient, such as for defining an expression:

```rust
let users = Table::new("users", postgres())
    .with_column("id")
    .with_column("name")
    .with_column("surname")
    .with_expression("full_name", |t| {
        expr!(
            "concat_ws({}, {}, {})",
            " ",
            t.get_column("name").unwrap(),
            t.get_column("surname").unwrap()
        )
    });
```

Columns can also be used in conditions:

```rust
let condition = users.get_column("name").unwrap().eq("John");
let john = user.with_condition(condition);
```

When you use `user.get()`, with `User { name, surname, full_name }`, `Table` needs to
ensure query would include both columns and expression too. More broadly, lets talk about
what can be deserialised into a `User` entity fields:

## Field could be:

1. **Column** - There is a physical SQL column `name` and `surname`.
2. **Expression** - No physical column, but `full_name` is defined through a SQL expression.
3. **Joined Columns** - Table could be joined with another table, combining columns.
4. **Callback** - Value is calculated after record is fetched using a callback.

Lets dive into the following example scenario:

1. `users: Table<Postgres, User>` has a column `name` and `surname`, using table "user"
2. I added an expression `full_name` that combines `name` and `surname` columns
3. I also joined "user_address" table, that contains `street` and `city` columns
4. I also define callback for calculating `post_code` by fetching from external API or cache.

After above operations the following is true:

- `users` has 3 columns
- `users` has 1 expression
- `users` has 1 joined table, which has another 2 columns
- `users` has 1 callback
- `users` has 6 fields: `id`, `name`, `surname`, `full_name`, `user_address_street`, `user_address_city`
- `users` can deserialise into `User` struct with 7 fields: `id`, `name`, `surname`, `full_name`, `user_address_street`,
  `user_address_city` and `post_code`

## Working with Table Columns: SqlTable

Column operations are implemented in `TableWithColumns` trait:

- `add_column` - adds a column to the table
- `columns` - returns all physical columns
- `add_id_column` and `add_title_column` - adds a column but also labels it
- `id` - return `id` column
- `title` - return `title` column
- `id_with_table_alias` - return `id` column with table alias
- `get_column` - return a column by name
- `get_column_with_table_alias` - return a column by name with table alias
- `search_for_field` - similar to `get_column` but will look for lazy expressions and columns from joined tables.

## Working with Table Columns: Table<D, E>

`Table<D, E>` implements some additional methods for convenience:

- `with_column` - adds a column to the table and returns it
- `with_title_column` - adds a title column to the table and returns it
- `with_id_column` - adds an id column to the table and returns it

```rust
let users = Table::new("users", postgres())
    .with_id_column("id")
    .with_title_column("name")
    .with_column("role_name");
```

## Extending Entity with column getters

it is common practice to define field getters like this:

```rust
pub trait ClientTable: SqlTable {
    fn name(&self) -> Arc<Column> {
        self.get_column("name").unwrap()
    }
    fn contact_details(&self) -> Arc<Column> {
        self.get_column("contact_details").unwrap()
    }
    fn bakery_id(&self) -> Arc<Column> {
        self.get_column("bakery_id").unwrap()
    }
}
```

This makes it easier to reference columns:

```rust
expr!(
    "concat_ws({}, {}, {})",
    " ",
    //t.get_column("name").unwrap(),
    //t.get_column("surname").unwrap()
    t.name(),
    t.surname()
)
```

## Conclusion

Ability to specify columns not by name but through a dedicated method makes
use of Rust type system and avoids typos at compile time. Fields in a query
can be defined through different means.

Swapping Column into Expression allow you to restructure your field names
without changing the code, also you can bring columns from across the
entities, but for that we will need to learn more about Expressions
