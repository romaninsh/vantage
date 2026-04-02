# What's new in 0.0.3

We are now starting major refactoring inside the `vantage` crate itself. A huge problem was the size of `Table` struct (exactly 1000 lines).
It's possible to repeat `impl Table` several times, and ability to include `mod` and `use` statemetns to group
things from multtiple files.

## Refactor "Query" to use `with_` methods

While I have renamed methods to use `with_` for Table, Query was still using `add_`. Now Query has been made
more consistent:

```rust
let query = Query::new()
    .with_table("users", None)
    .with_column_field("id")
    .with_column_field("name")
    .with_condition(expr1)
    .with_condition(expr2);
```

## New entity pattern with `Product::table()`

```rust
pub struct Product {}
impl Product {
    pub fn table() -> Table<Postgres> {
        Product::static_table().clone()
    }
    pub fn static_table() -> &'static Table<Postgres> {
        static TABLE: OnceLock<Table<Postgres>> = OnceLock::new();

        TABLE.get_or_init(|| {
            Table::new("product", postgres())
                .with_id_field("id")
                .with_field("name")
        })
    }

    pub fn name() -> Arc<Field> {
        Product::static_table().get_field("name").unwrap()
    }
}
```

This is a minor change from before - instead of `ProductSet::new()` we are now using `Product::table()`.
The implementation will continue to evolve a bit more, but this is pretty much where we are now with
the Entity paradigm.

## Adedd 11 chapters into the documentation book

Restructure documentation to cover all the features - implemented and not yet implemented.

Book provides awesome guidance on what to implement next.

## Introduction of `Entity` trait #8

Previously, table type was defined as `Table<Postgres>`. Because I am sick of static field methods (ProductSet::name)
I am adding second generic parameter to `Table`:

```rust
let product: Table<Postgres, Product> = Table::new_with_entity("product", postgres());
```

There is also `EmptyEntity` which is a struct without fields and is returned by `Table::new()`. The new paradigm
allows me to define table fields by implementing a trait for Table<Postgres, Entity>.

In order to make this work, the `Table` struct which was implemented as a monolith this far, is now starting
to split up into multiple traits, each implementing a part of table functionality. I am also taking care
to keep those traits Dyn-safe.

At this point only `AnyTable` trait is created, but many more will follow.
