# Step 2b: Implement Operators

Expressions let you build raw queries, but users shouldn't have to write
`sqlite_expr!("{} > {}", (ident("price")), 100i64)` every time they want a condition. **Operators**
give typed columns ergonomic methods like `.eq()`, `.gt()`, `.in_()` that produce your backend's
native condition type.

This step covers how to implement a vendor-specific operation trait for your persistence.

### Vendor-specific operation traits

Each persistence defines its own operation trait that returns the backend's condition type directly.
The trait is blanket-implemented for all `Expressive<T>` where `T: Into<AnyBackendType>`, so typed
columns get the methods for free.

For SQL backends, a macro generates the trait:

```rust
// In vantage-sql/src/sqlite/operation.rs
define_sql_operation!(
    SqliteOperation,
    SqliteCondition,
    crate::sqlite::types::AnySqliteType
);
```

This produces:

- A trait `SqliteOperation<T>` with `.eq()`, `.gt()`, `.lt()`, `.ne()`, `.gte()`, `.lte()`,
  `.in_()`, `.in_list()`, `.cast()` — all returning `SqliteCondition`
- A blanket impl for all `Expressive<T>` where `T: Into<AnySqliteType>`
- An `Expressive<AnySqliteType>` impl for `SqliteCondition`, enabling chaining

### How it works internally

Each method builds an `Expression<T>` from the two operands, then converts it to the backend's
condition type via `From<Expression<T>>`:

```rust
fn gt(&self, value: impl Expressive<T>) -> SqliteCondition {
    let expr: Expression<T> = Expression::new("{} > {}", vec![
        ExpressiveEnum::Nested(self.expr()),
        ExpressiveEnum::Nested(value.expr()),
    ]);
    SqliteCondition::from(expr)  // maps T → AnySqliteType via Into
}
```

The `From<Expression<F>> for SqliteCondition` impl (from Step 1's `define_sql_condition!` macro)
handles the type mapping — it calls `ExpressionMap::map()` to convert all `F` scalars into
`AnySqliteType`.

### Chaining across type boundaries

Because `SqliteCondition` implements `Expressive<AnySqliteType>`, the blanket gives it
`SqliteOperation<AnySqliteType>`. This enables:

```rust
let price = Column::<i64>::new("price");
price.gt(10).eq(false)
// => SqliteCondition wrapping: (price > 10) = 0
```

The first operation (`.gt(10)`) enforces type safety — `10` must be `Expressive<i64>`. The second
operation (`.eq(false)`) operates on `SqliteCondition` where `bool: Expressive<AnySqliteType>`, so
any backend-compatible type is accepted.

### Implementing for a non-SQL backend

For backends that don't use expression trees for conditions (like MongoDB), you implement the
operation trait manually instead of using the macro. MongoDB produces BSON documents:

```rust
pub trait MongoOperation<T>: Expressive<T> {
    fn eq(&self, value: impl Into<AnyMongoType>) -> MongoCondition {
        let field = self.expr().template.clone();
        let bson_val = AnyMongoType::from(value).to_bson();
        MongoCondition::Doc(doc! { field: { "$eq": bson_val } })
    }

    fn gt(&self, value: impl Into<AnyMongoType>) -> MongoCondition {
        let field = self.expr().template.clone();
        let bson_val = AnyMongoType::from(value).to_bson();
        MongoCondition::Doc(doc! { field: { "$gt": bson_val } })
    }
    // ...
}

impl<T, S: Expressive<T>> MongoOperation<T> for S {}
```

Key differences from SQL:

- **Values use `Into<AnyMongoType>`** not `Expressive<T>` — MongoDB doesn't compose expression
  trees, it builds BSON documents from scalar values.
- **Field name extraction** — `self.expr().template` gives the column name for simple columns.
  Complex expressions produce the template string as the field path.
- **Chaining** — `MongoCondition` implements `Expressive<AnyMongoType>` for the blanket, but boolean
  chaining (`.eq(false)` = negate) is handled via dedicated methods like `.eq_bool(false)` since
  MongoDB negation uses `$not` wrappers.

### Avoiding method name conflicts

When multiple backend features are enabled, types like `Identifier` and `&str` implement
`Expressive<T>` for multiple backends. This causes ambiguity if the operation trait is generic.

Each backend's operation trait lives in its own module (e.g. `sqlite::operation::SqliteOperation`).
Users import only the trait they need:

```rust
// In your prelude:
pub use crate::sqlite::operation::SqliteOperation;
```

### Condition type requirements

Your condition type must satisfy `TableSource::Condition` bounds — `Clone + Send + Sync + 'static`.
It also needs:

- `From<Expression<F>>` for any `F: Into<AnyType>` — so typed column operations convert cleanly
- `From<Identifier>` — so `ident("field")` works with `with_condition()`
- `Expressive<AnyType>` — so the condition can be chained with further operations
- Any backend-specific conversions (e.g. `From<Document>` for MongoDB)

For SQL backends, the `define_sql_condition!` macro generates all of these.

### Step 2b checklist

1. **Define your operation trait** — either via `define_sql_operation!` (SQL) or manually
   (document-oriented backends).

2. **Blanket-implement it** for all `Expressive<T>` where `T` converts into your `AnyType`.

3. **Implement `Expressive<AnyType>` for your condition type** — enables chaining.

4. **Export from your prelude** — so users get the operation trait automatically.

5. **Tests** covering:
   - Typed column operations: `Column::<i64>::new("price").gt(150)` → condition
   - Boolean column: `Column::<bool>::new("active").eq(false)` → condition
   - Chaining: `price.gt(10).eq(false)` compiles and produces correct output
   - Cross-type rejection: `price.gt(false)` does NOT compile
   - Same-type column comparison: `price.eq(price.clone())` works
   - Condition usable with `table.add_condition()` and `select.with_condition()`
