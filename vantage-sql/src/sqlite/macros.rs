/// Create a SQL expression with `serde_json::Value` as the value type.
///
/// This is the untyped variant — parameters are plain JSON values without
/// type markers. Useful for statement builders (Step 3) where types are
/// inferred from context.
///
/// - `sql_expr!("template")` — no parameters
/// - `sql_expr!("template", scalar)` — scalar becomes `ExpressiveEnum::Scalar(scalar.into())`
/// - `sql_expr!("template", (expr))` — expr becomes `ExpressiveEnum::Nested(expr.expr())`
#[macro_export]
macro_rules! sql_expr {
    ($template:expr) => {
        vantage_expressions::Expression::<serde_json::Value>::new($template, vec![])
    };

    ($template:expr, $($param:tt),*) => {
        vantage_expressions::Expression::<serde_json::Value>::new(
            $template,
            vec![
                $(
                    vantage_expressions::expr_param!($param)
                ),*
            ]
        )
    };
}

/// Create a SQL expression with `AnySqliteType` as the value type.
///
/// This is the typed variant — parameters carry SQLite type markers (Integer,
/// Text, Real, etc.) which are used for type-aware binding via `bind_sqlite_value`.
/// Use this when building expressions for `ExprDataSource::execute()`.
///
/// Scalar arguments must implement `Into<AnySqliteType>` — supported types are:
/// `i8`–`u32`, `f32`/`f64`, `bool`, `String`, and `&str`.
///
/// ```ignore
/// let expr = sqlite_expr!("SELECT * FROM product WHERE price > {}", 100i64);
/// let result = db.execute(&expr).await?;
/// ```
#[macro_export]
macro_rules! sqlite_expr {
    ($template:expr) => {
        vantage_expressions::Expression::<$crate::sqlite::AnySqliteType>::new($template, vec![])
    };

    ($template:expr, $($param:tt),*) => {
        vantage_expressions::Expression::<$crate::sqlite::AnySqliteType>::new(
            $template,
            vec![
                $(
                    vantage_expressions::expr_param!($param)
                ),*
            ]
        )
    };
}
