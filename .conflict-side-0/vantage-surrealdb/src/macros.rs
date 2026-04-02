/// Create a SurrealDB expression with automatic type conversion
///
/// Uses `expr_param!` from vantage-expressions for parameter handling:
/// - `surreal_expr!("template")` — no parameters
/// - `surreal_expr!("template", scalar)` — scalar becomes `ExpressiveEnum::Scalar(scalar.into())`
/// - `surreal_expr!("template", (expr))` — expr becomes `ExpressiveEnum::Nested(expr.expr())`
/// - `surreal_expr!("template", {deferred})` — deferred becomes `ExpressiveEnum::Deferred(deferred)`
///
/// Any type implementing `SurrealType` can be used as a scalar (i64, String, bool, etc.)
/// since `From<T: SurrealType> for AnySurrealType` is implemented as a blanket.
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
