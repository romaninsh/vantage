/// Create a SQL expression with `AnyPostgresType` as the value type.
///
/// This is the typed variant -- parameters carry PostgreSQL type markers (Int4,
/// Text, Float8, etc.) which are used for type-aware binding via `bind_postgres_value`.
///
/// Scalar arguments must implement `Into<AnyPostgresType>` -- supported types are:
/// `i8`-`u32`, `f32`/`f64`, `bool`, `String`, and `&str`.
///
/// ```ignore
/// let expr = postgres_expr!("SELECT * FROM product WHERE price > {}", 100i64);
/// let result = db.execute(&expr).await?;
/// ```
#[macro_export]
macro_rules! postgres_expr {
    ($template:expr) => {
        vantage_expressions::Expression::<$crate::postgres::AnyPostgresType>::new($template, vec![])
    };

    ($template:expr, $($param:tt),*) => {
        vantage_expressions::Expression::<$crate::postgres::AnyPostgresType>::new(
            $template,
            vec![
                $(
                    vantage_expressions::expr_param!($param)
                ),*
            ]
        )
    };
}
