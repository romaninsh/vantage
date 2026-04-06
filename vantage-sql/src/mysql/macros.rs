/// Create a SQL expression with `AnyMysqlType` as the value type.
///
/// Scalar arguments must implement `Into<AnyMysqlType>` -- supported types are:
/// `i8`-`u32`, `f32`/`f64`, `bool`, `String`, and `&str`.
///
/// ```ignore
/// let expr = mysql_expr!("SELECT * FROM product WHERE price > {}", 100i64);
/// let result = db.execute(&expr).await?;
/// ```
#[macro_export]
macro_rules! mysql_expr {
    ($template:expr) => {
        vantage_expressions::Expression::<$crate::mysql::AnyMysqlType>::new($template, vec![])
    };

    ($template:expr, $($param:tt),*) => {
        vantage_expressions::Expression::<$crate::mysql::AnyMysqlType>::new(
            $template,
            vec![
                $(
                    vantage_expressions::expr_param!($param)
                ),*
            ]
        )
    };
}
