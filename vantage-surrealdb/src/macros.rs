/// Create a SurrealDB expression with automatic type conversion
///
/// Uses `expr_param!` from vantage-expressions for parameter handling:
/// - `surreal_expr!("template")` — no parameters
/// - `surreal_expr!("template", scalar)` — scalar becomes `ExpressiveEnum::Scalar(scalar.into())`
/// - `surreal_expr!("template", (expr))` — expr becomes `ExpressiveEnum::Nested(expr.expr())`
/// - `surreal_expr!("template", {deferred})` — deferred becomes `ExpressiveEnum::Deferred(deferred)`
///
/// Scalar arguments must implement `Into<AnySurrealType>` — supported types are:
/// `i8`–`u64`, `isize`/`usize`, `f32`/`f64`, `bool`, `String`, and `&str`.
/// Other expression-like values should be wrapped in `( ... )` for nested expressions.
#[macro_export]
macro_rules! surreal_expr {
    // Simple template without parameters
    ($template:expr) => {
        vantage_expressions::Expression::<$crate::AnySurrealType>::new($template, vec![])
    };

    // Template with parameters
    ($template:expr, $($param:tt),*) => {
        vantage_expressions::Expression::<$crate::AnySurrealType>::new(
            $template,
            vec![
                $(
                    vantage_expressions::expr_param!($param)
                ),*
            ]
        )
    };
}
