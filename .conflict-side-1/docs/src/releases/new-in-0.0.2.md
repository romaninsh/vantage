# What's new in 0.0.2

Bringing references, lazy expressions and refactoring Field into Arc<Field>.

## Added `has_one` and `has_many` methods

This is my first experience with Fn's. When defining a relationship, table must be able to
return related table:

```rust
struct PersonSet {
    table: Table<MockDataSource>,
}
impl PersonSet {
    fn table() -> Table<MockDataSource> {
        let data = json!([]);
        let db = MockDataSource::new(&data);
        let mut table = Table::new("persons", db)
            .with_field("id")
            .with_field("name")
            .with_field("parent_id")
            .has_one("parent", "parent_id", || PersonSet::table())
            .has_many("children", "parent_id", || PersonSet::table());
        table
    }
}
```

```rust
let grand_children = john
    .get_ref("children")
    .unwrap()
    .get_ref("children")
    .unwrap();
```

generating this SQL:

```sql
"SELECT id, name, parent_id FROM persons WHERE \
(parent_id IN (SELECT id FROM persons WHERE \
(parent_id IN (SELECT id FROM persons WHERE (name = {})\
))\
))"
```

## Lazy Expressions

```rust
let mut orders = Table::new("orders", db.clone())
    .with_field("price")
    .with_field("qty");
orders.add_expression_before_query("total", |t| {
    expr_arc!(
        "{}*{}",
        t.get_field("price").unwrap().clone(),
        t.get_field("qty").unwrap().clone()
    )
    .render_chunk()
});
let query = orders.get_select_query().render_chunk().split();

assert_eq!(query.0, "SELECT price, qty, price*qty AS total FROM orders");
```

This example actually does not work, becaues `get_select_query()` does not query for expressions by default.

## Field is now Arc<Field>

My favourite feature of Rust is that it allows implementing traits not only for structs, but for any type.
While `Arc` is not defined by me, I can implement more methods for `Arc<T> where T: Field` and it will
work everywhere.

Previously Table's `get_field()` would return a &Field. The problem with this approach is that field comes
with a lifetime, and it cannot exist without a table. By using `Arc` instead, fields do not actually care
if table is still there or not.

This is more convenient for us, so all field methods are now returning `Arc<Field>` and I implemented
`Chunk` and `Column` traits for `Arc<Field>` rather than `Field`.

## `testcontainers` trying for a while

I thought it would be interesting if rust tests would automatically spin up a container with postgres.
While it was working locally, I could not figure out how to make it work in CI and therefore `testcontainers`
were dropped later on.

## restructuring into crates

`vantage`, `bakery_model` and `docs` are now three folders in a root of a repository. Keeping bakery model out
so that if someone is to use `vantage` - they wouldn't need to deal with `bakery_model'.

## Added CI process

When PR is raised in GitHub repository, it compiles the lib, runs unit tests and also renders documentation
and stores it as github pages.
