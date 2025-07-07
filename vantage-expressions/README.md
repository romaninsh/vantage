# Implementing Expressions

Macro `expr!` allows you to easily create simple expressions, using scalar
values as parameters. Expression is not associated with any database or
language syntax and therefore is agnostic.

```rust
let expr = expr!("select {} + {}", 2, 4);
```

Expressions can be `associate()` with a datasource:

```rust
ds = Arc::new(DataSource::new());
let associated_expr = ds.associate(expr);
let data = associated_expr.get().await;
```

Associated expression carries reference to your data source and can be
executed at will for instance if you call `expr.get().await`.

There are also Lazy Expressions, that can reference bunch of various things:

```rust
let expr = expr!("select id form user where is_enabled");
let lazy_expr = lazy_expr!("select * from data where user_id in {}", expr);
```

Here lazy_expr
