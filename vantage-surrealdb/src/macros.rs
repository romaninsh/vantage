//! SurrealDB-specific expression macros
//!
//! These macros provide convenient ways to create expressions with AnySurrealType
//! without requiring From trait implementations.

/// Create a SurrealDB expression with automatic AnySurrealType wrapping
///
/// Usage:
/// - `surreal_expr!("template")` - no parameters
/// - `surreal_expr!("template", arg1, arg2)` - wraps args in AnySurrealType::new()
/// - `surreal_expr!("template", (expr))` - nested expression (no wrapping)
/// - `surreal_expr!("template", {deferred})` - deferred function (no wrapping)
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
                    $crate::surreal_param!($param)
                ),*
            ]
        )
    };
}

/// Helper macro to handle different parameter types for SurrealDB expressions
#[macro_export]
macro_rules! surreal_param {
    // Nested expression: (expr) -> ExpressiveEnum::Nested(expr)
    (($expr:expr)) => {
        vantage_expressions::ExpressiveEnum::Nested({
            use vantage_expressions::Expressive;
            $expr.expr()
        })
    };

    // Deferred function: {fn} -> ExpressiveEnum::Deferred(fn)
    ({$deferred:expr}) => {
        vantage_expressions::ExpressiveEnum::Deferred($deferred)
    };

    // Regular scalar: expr -> ExpressiveEnum::Scalar(AnySurrealType::new(expr))
    ($param:expr) => {
        vantage_expressions::ExpressiveEnum::Scalar($crate::AnySurrealType::new($param))
    };
}
