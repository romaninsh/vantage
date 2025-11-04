# What's new in 0.0.1

The 0.0.1 includes all the changes, that were released in the initial commits, so this is very approximate.

## Query Building

Added Query struct with `set_type`, `fields`, `field` and `build`.

```rust
let query = Query::select()
```

## Introduced `Renderable` trait (later renamed into `Chunk`)

Renderable trait can be part of a query.

## Introduced `Field` struct

Contains name and alias.

## Introduced multiple Query types

- `Select`
- `Insert`
- `Update`
- `Delete`

Query render also renders fields.

## Introduced `Expression` struct

It would be very difficult for Query to render everything, so `Expression` is a basic template
engine that carries parts of SQL code and separate parameters:

```rust
let expression = expr!("{} + {}", "3", "5");
```

## Fields positional rendering

Fields can render differently. If field is a part of "SELECT {} FROM", it may include "AS" clause, but
if field is mentioned elsewhere, it may include table name. That's why I introduced `Column` trait also.
in addition to `Chunk` (former `Renderable`) trait.

## Added initial concept for `ReadableDataSet` and `WritableDataSet`

Methods for generating qureies for each are added as placeholders, but will remain unimplemented until
much later.

## Added `mocking` for unit tests

To keep tests robust, I introduced `mocks`. A mock datasource has a vec of data, which it would
simply return on all queries.

## Added `sqlite` (rustlite) integration tests

Before settling on `postgres` a brief implementation for `sqlite` was added.

```rust
let mut query = Query::new("person");
query.add_column_expr(expr!("name || ' ' || surname"));
let result = conn.query_fetch(&query).unwrap();
```

## Death by lifetimes

In early implementations I thought that `Query` would have a lifetime, that would based around a lifetime of
the table. It was a horrible idea.

`expr!` macro was slightly tweaked, but it still works only with strings.

```rust
let a = "3".to_owned();
let b = "5".to_owned();
let expression = expr!("{} + {}", &a, &b);
```

Table cleaned up, but initial method naming is very inconsistent.

```rust
let table = Table::new("users", Box::new(data_source))
    .add_field("name")
    .add_field("surname")
    .add_condition("name", "=", "John".to_owned());

assert_eq!(
    table.select_query().render(),
    "SELECT name, surname FROM users WHERE name = 'John'".to_owned()
);
```

## Added Conditions

Initial implementation of conditions is very limited - no way to specify a field. A new implementation
is going to store field reference, operation and value.

## Apr 18: Name changed to `dorm` and early syntax added in README:

This is how early syntax looked like:

```rust
let users = User::new(datasource);
let orders = users
    .filter(users.last_online().gt(thirty_minutes_ago))
    .ref_orders();
let results = orders
    .filter(orders.is_active().is(false))
    .query_expr(Expression::concat(
        'Active user ',
        user.email(),
        'has a basket worth ',
        Expression::sum(orders.amount()
    )).iter_col()
```

`.filter` and the use of `users` is inspired by Diesel, however field names are defined as methods. `User` struct
holds a `table` and implements Deref.

The definition of `User` also supports field types:

```rust
let table = Table::new("product", ds)
    .add_field(Field::new("id", Type::Serial).primary())
    .add_field(Field::new("name", Type::Varchar(255)).not_null())
    .add_field(Field::new("description", Type::Text))
    .add_field(Field::new("price", Type::Decimal(10, 2)).not_null());
```

Ditched `sqlite` and used `postgres` instead. Implementation of type casting is pain. Query rendering for insert/select. is implemented.

## Nested expressions

Early expressions could only hold values, they couldn't hold other expression. Now is a good time to fix this
by implementing `SqlChunk` trait (replaces `Renderable`). There are some problems with keeping those expressions
in a vec dynamically, but it's a start!

```rust
let nested = Expression::new("Nested".to_string(), vec![]);
let expression =
    Expression::new("Hello {} World".to_string(), vec![&nested as &dyn SqlChunk]);
```

Expressions capable of storing other expressions is initially called "PreRender", but
will be later renamed into "ExpressionArc".

```rust
let pre_render1 = expr!("{} + {}", 1, 2).render_chunk();
let pre_render2 = expr!("{} + {}", 3, 4).render_chunk();
let pre_vec = vec![pre_render1, pre_render2];
let join = PreRender::from_vec(pre_vec, " + ");
assert_eq!(join.sql, "{} + {} + {} + {}");
```

## Operations

Operations is a trait that allows multiple types to wrap themselves into a simple expression:

```rust
let f_age = Field::new("age".to_string());
let expr = field.add(5).eq(18);
```

## Lifetime hell. Arc() and Box() to the rescue

To better pass data around, wrapping them in a Box is often a good idea. Better yet - use Arc too.
I suggest to learn about Box and Arc early and if you use traits, be sure to read about dyn-safety.
Not understanding this would take a lot of your time.

## Added generics with <T: DataSource>

Because we realise that datasource implementations could be different, a DataSource is now a generics
in a Table struct.

## Huge refactoring in Rust

I must say that doing a massive refactoring in Rust is much more fun than other languages. As soon
as you fix all the syntax error, code just works!

Renamed `PreRender` into `Expression_Arc` finally and added `expr_arc!` macro.

I could tick off "properly implement nested queries".

## Added `Table.sum()`

```rust
let product_set = ProductSet::new(postgres.clone());

let product_with_certain_price = product_set.add_condition(
    product_set
        .price()
        .gt(Decimal::new(10, 0))
        .and(product_set.price().lt(Decimal::new(100, 0))),
);
let product_price_sum = product_set.sum(product_set.price()).get_one().await?;
let special_price_sum = product_with_certain_price
    .sum(product_set.price())
    .get_one()
    .await?;
```

## Refactor of Conditions

Previously condition was quite clumsy, now it has multiple options for the operand:

- can be another field
- can be expression
- could be another condition
- could be static value

Also probably at this time Operations (such as field.eq(5)) started returning Condition rather
then expression. The trick here is that some conditions may need to be changed if the alias is
set for the table.

## Added `AssociatedExpressionArc` and `AssociatedQuery`

This is the expression that is linked to a datasource. Both of those associated types implement
deref trait, and simply add a method `fetch`. This is handy, because you can execute this into
result without any extra input.

## Added `Query.join()`

In order for joins to work, we need to first figure out how to generate aliases for table. Join
can create ambiguity. I have implemented `UniqueIdVendor` generator for unique identifiers. It
takes name of the table into account and will shorten it as well as add `_1`, `_2` at the end
if duplicated.

## Refactored and finished `Query`

When I tell others I have built a query builder, they ask - can it build any query?

Well - yes, by definition `expression` can be used for any query, but the `Query` class itself
must be capable of many things. For instance, it sholud be able to use `WITH` clause, or use
select as a source of a query.

To test this out, I have implemented a massively complex query. It required me to significantly
refactor Query code by adding many new features.

Obviously, when `Table` is used to create a query, it won't use many of tohse features.

## Added `mdbook` documentation

I often find that writing documentation makes me rethink my design. I have added `mdbook` documentation
into the repository and started writing it:

```rust
let query = Query::new()
    .set_table("users")
    .add_column_field("id")
    .add_column_field("email")
    .add_column("full_name", Some(expr!("concat(first_name, ' ', last_name)")))
    .add_where(expr!("age > {}", 18))
    .add_order_by(expr!("full_name"))
```

Right away this sparks more ideas. I am also able to describe a power of a new Query engine:

```rust
let roles = Query::new()
    .set_table("roles")
    .add_column_field("id")
    .add_column_field("role_name");
let outer_query = Query::new()
    .set_table("users")
    .add_with("roles", roles)
    .add_join(JoinQuery::new(
        JoinType::Inner,
        QuerySource::Table("roles".to_string(), None),
        QueryConditions::on().add_condition(expr!("users.role_id = roles.id")),
    ))
    .add_column_field("user_name")
    .add_column_field("roles.role_name");
let group = Query::new()
    .set_source(QuerySource::Query(outer_query))
    .add_column_field("role_name")
    .add_column_field("c", Operation::count("*"))
    .add_group_by("role_name");
```

The design for Entities is also documented, although it won't be long-lived:

```rust
pub struct ProductSet {
    table: Table<Postgres>,
}
impl ProductSet {
    pub fn new(ds: Postgres) -> Self {
        let table = Table::new("product", ds)
            .add_field("name")
            .add_field("description")
            .add_field("default_price");
        Self { table }
    }
    pub fn name(&self) -> &Field {
        self.get_field("name").unwrap()
    }
    pub fn description(&self) -> &Field {
        self.get_field("description").unwrap()
    }
    pub fn price(&self) -> &Field {
        self.get_field("default_price").unwrap()
    }
}
```

## Static entities

Until this point, every time `Product::new()` is called, a new table is created. It would be easier,
if the product table was static and would simply be cloned when needed. I started looking into lazy_static
and OneCell to find a best solution for this.

## Added Bakery example and use clone everywhere

It wolud be nice to have an example as a part of `vantage` itself and reference example entities in the documentation.
Bakery was already partially there, but now it got extended, and ER diagram is also included.

DataSource is now OnceLock and is cloned every time `postgres()` is called.

Structs are changed and BakerSet::new() would call BakerSet::table() which initializes table once and returns references,
which new() would clone.

Fields are now defined as static methods, instead of `ProductSet::new().name()` you should use `ProductSet::name()`.

## Second attempt at table joining

Significantly improved joining and use of aliases.

```rust
let user_table = Table::new("users", db.clone())
    .set_alias("u")
    .add_field("name")
    .add_field("role_id");
let role_table = Table::new("roles", db.clone())
    .add_field("id")
    .add_field("role_description");
let table = user_table.join_table(role_table, "id", "role_id");
```

## Consistency for naming `with_` and `add_`

For a long time I coludn't decide a best pattern for naming, now I have implemented both methods: `with_` and `add_`.
The difference is that `add_` is dyn-safe and is used internally. The `with_` is a syntax sugar for `add_` and
is given to the user for convenience. The call signature sometimes differs, and `with_` being simpler case.

For example `with_field`, `with_id_field` and `with_title_field` all are using `add_field`.

## More improvements for joins

Unique identifier generator now tracks which names to properly avoid and can be shared and even merged. This is useful
when more than 2 tables are joined.

```rust
let person = Table::new("person", db.clone())
    .with_field("id")
    .with_field("name")
    .with_field("parent_id");
let father = person.clone().with_alias("father");
let grandfather = person.clone().with_alias("grandfather");
let person = person.with_join(father.with_join(grandfather, "parent_id"), "parent_id");
let query = person.get_select_query().render_chunk().split();

        assert_eq!(
            query.0,
            "SELECT p.id, p.name, p.parent_id, \
            father.id AS father_id, father.name AS father_name, father.parent_id AS father_parent_id, \
            grandfather.id AS grandfather_id, grandfather.name AS grandfather_name, grandfather.parent_id AS grandfather_parent_id \
            FROM person AS p \
            LEFT JOIN person AS father ON (p.parent_id = father.id) \
            LEFT JOIN person AS grandfather ON (father.parent_id = grandfather.id)"
        );
```

What a massive yet beautiful query!

```rust
let mut user_table = Table::new("users", db.clone())
    .with_field("name")
    .with_field("role_id");
let role_table = Table::new("roles", db.clone())
    .with_field("id")
    .with_field("role_type");
let join = user_table.add_join(role_table, "role_id");
user_table.add_condition(join.get_field("role_type").unwrap().eq(&json!("admin")));
```

We can even set queries to fields from join.

Can we call this a 0.0.1 yet?
