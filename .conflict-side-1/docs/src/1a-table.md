# Table

I have introduced `sql::Table` in context of [Data Sets](./1-dataset.md), however before
diving deep into `Table` I must introduce `SqlTable` trait - a dyn-safe version of `Table`.

`Table` type takes 2 generic parameters: `DataSource` and `Entity`. This is similar to
`Vec<T>` where `T` is a generic parameter. In other words `Table<Postgres, User>` is
not the same as `Table<Postgres, Order>`.

`Table` also have methods returning `Self` for example `with_column`:

```rust
let users = Table::new("users", postgres());
let users = users.with_column("id"); // users has same type
```

Generic parameters and methods that return `Self` cannot be defined in dyn-safe traits.
(See <https://doc.rust-lang.org/reference/items/traits.html#object-safety> for more info),
whihc is why I created `sql::SqlTable` trait.

`Table` gives you convenience and you get unique type for your business entities,
but if you need to deal with generic `Table` you can use `SqlTable` trait:

```rust
fn get_some_table() -> Box<dyn SqlTable> {
    if some_condition() {
        Box::new(Table::new("users", postgres()))
    } else {
        Box::new(Table::new("orders", postgres()))
    }
}
```

To convert `Box<dyn SqlTable>` back to `Table<Postgres, User>`, you can use downcasting:

```rust
let user: Table<Postgres, User> = get_some_table().as_any_ref().downcast_ref().unwrap();
```

If `some_condition()` was false and "orders" table was returned you get type missmatch and downcast
will fail. That's just how Rust type system works, so only downcast when you are 100% sure that you
are using the right type.

This works really for defining custom `ref_*` methods for entity traversal:

```rust
fn ref_orders(users: &Table<Postgres, User>) -> Table<Postgres, Order> {
    users.get_ref("orders").unwrap().as_any_ref().downcast_ref().unwrap()
    //       ^ returns Box<dyn SqlTable>
}
```

There is a more convenient method `get_ref_as`, that would downcast it for you:

```rust
fn ref_orders(users: &Table<Postgres, User>) -> Table<Postgres, Order> {
    users.get_ref_as("orders").unwrap()
    //       ^ returns Table<Postgres, _>
}
```

Let me collect some facts about `Table` and `SqlTable`:

1. `sql::SqlTable` is a dyn-safe trait implementing most basic features of a table
2. `sql::Table` is a struct implementing `SqlTable` trait and some additional features (such as ReadableDataSet)
3. When Table must refer to another table in a generic way, it will be using `dyn SqlTable`
4. `sql::Table` type relies on 2 generic parameters: `DataSource` and `Entity`
5. `DataSource` describes your SQL flavour and can affect how queries are built etc.
6. `Entity` can be implemented by any struct.

To reinforce your understanding of how this all works together, lets compare 3 examples.
First I define a function that generates a report for `Table<Postgres, Order>`:

```rust
fn generate_order_report(orders: &Table<Postgres, Order>) -> Result<String> {
    ...
}

generate_order_report(Order::table()); // Table<Postgres, Order>
// generate_order_report(Client::table());
// does not compile ^
```

I'd like to test my method using `MockDataSource` and therefore I want it to work with
any `DataSource`:

```rust
async fn generate_order_report<D: DataSource>(orders: Table<D, Order>) -> Result<String> {
    ...
}

let orders = Order::mock_table(&mock_orders);
generate_any_report(orders).await?;  // Table<MockDataSource, Order>

```

What if my code should work with any entity, but I don't wish to deal with `SqlTable`?

```rust
fn generate_any_report<D: DataSource, E: Entity>(table: Table<D, E>) -> Result<String> {
    ...
}

generate_any_report(Order::table()).await?;  // Table<Postgres, Order>
generate_any_report(Client::table()).await?; // Table<Postgres, Client>

let orders = Order::mock_table(&mock_data);
generate_any_report(orders).await?;  // Table<MockDataSource, Order>
```

(The nerdy explanation here is that Rust compiler will create 3 copies of `generate_any_report`
function for each `D` and `E` combinations you use in the code).

## Creating

A simplest way to create a table object would be Table::new:

```rust
let users = Table::new("users", postgres())
```

The type of `users` variable shall be `Table<Postgres, EmptyEntity>`. If instead of `EmptyEntity`
you'd like to use `User` you can use `new_with_entity` method.

`DataSource` type is inferred from the second argument to new() - return type of
`postgres()` function.

Lets look at how we can define our own Entity:

```rust
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct User {
    id: i64,
    name: String,
}
impl Entity for User {}

let users = Table::new_with_entity::<User>("users", postgres());
```

Rust will infer type when it can, so:

```rust
fn user_table() -> Table<Postgres, User> {
    Table::new_with_entity("users", postgres())
}
```

Finally, rather than implementing a stand-alone method like that, we can implement it on the `User`
struct:

```rust
impl User {
    fn table() -> Table<Postgres, User> {
        Table::new_with_entity("users", postgres())
    }
}
```

Since table structure is the same throughout the application, lets add columns
into the table:

```rust
impl User {
    pub fn table() -> Table<Postgres, User> {
        Table::new_with_entity("users", postgres())
            .with_column("id")
            .with_column("name")
    }
}
```

## Implementing custom traits for the entity

In Rust you can define an arbitrary trait and implement it on any type.
Lets define trait `UserTable` and implement it on `Table<Postgres, User>`:

```rust
trait UserTable: SqlTable {
    fn name(&self) -> Arc<Column> {
        self.get_column("name").unwrap()
    }
}
impl UserTable for Table<Postgres, User> {}
```

Now we can call `name()` method on type `Table<Postgres, User>` to access `name` column more directly:

```rust
let user = User::table();
let name_column = user.name();
```

We can also modify our `generate_order_report()` function into a custom trait:

```rust
#![allow(async_fn_in_trait)]
pub trait OrderTableReports {
    async fn generate_report(&self) -> Result<String>;
}

// was: async fn generate_order_report<D: DataSource>(orders: Table<D, Order>) -> Result<String>

impl<D: DataSource> OrderTableReports for Table<D, Order> {
    async fn generate_report(&self) -> Result<String> {
        ...
    }
}
```

## Conclusion

I have explained about `sql::Table` struct and `SqlTable` trait and which of the two should be used.
Also I have explained how to create custom traits and extend `Table` for specific entity type.

In my next chapters I'll refer to `Table` but in most cases you should understand that most
features would also work with `SqlTable` trait.

This chapter was Rust-intensive, but you should now understand how entity types are used in Vantage.
