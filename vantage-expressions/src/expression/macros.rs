/// Macro to create expressions with template and parameters for any type T
/// Syntax:
/// - expr_as!(T, "template", arg) -> arg becomes Scalar
/// - expr_as!(T, "template", (expr)) -> expr becomes Nested
/// - expr_as!(T, "template", {deferred}) -> deferred becomes Deferred
#[macro_export]
macro_rules! expr_as {
    ($t:ty, $template:expr) => {
        $crate::expression::core::Expression::<$t>::new($template, vec![])
    };

    ($t:ty, $template:expr, $($param:tt),*) => {
        $crate::expression::core::Expression::<$t>::new(
            $template,
            vec![
                $(
                    $crate::expr_param!($param)
                ),*
            ]
        )
    };
}

/// Macro to create expressions where type is inferred from context
/// Syntax:
/// - expr_any!("template", arg) -> arg becomes Scalar
/// - expr_any!("template", (expr)) -> expr becomes Nested
/// - expr_any!("template", {deferred}) -> deferred becomes Deferred
///
/// Function calls like `f(x)` work as scalar parameters without extra wrapping.
#[macro_export]
macro_rules! expr_any {
    ($template:expr) => {
        $crate::expression::core::Expression::new($template, vec![])
    };

    ($template:expr, $($param:tt),*) => {
        $crate::expression::core::Expression::new(
            $template,
            vec![
                $(
                    $crate::expr_param!($param)
                ),*
            ]
        )
    };
}

/// Macro to create expressions with serde_json::Value as default type
/// Syntax:
/// - expr!("template", arg) -> arg becomes Scalar
/// - expr!("template", (expr)) -> expr becomes Nested
/// - expr!("template", {deferred}) -> deferred becomes Deferred
#[macro_export]
macro_rules! expr {
    ($template:expr) => {
        $crate::expr_as!(serde_json::Value, $template)
    };

    ($template:expr, $($param:tt),*) => {
        $crate::expr_as!(serde_json::Value, $template, $($param),*)
    };
}

/// Helper macro to handle different parameter syntaxes.
///
/// - `(expr)` — nested expression (calls `.expr()` via `Expressive`)
/// - `{expr}` — deferred function
/// - `expr`   — scalar (calls `.into()`)
///
/// Function calls like `id_value(id)` work as scalars because `$($rest:tt)*`
/// captures the entire token sequence before forwarding to the scalar arm.
#[macro_export]
macro_rules! expr_param {
    (($expr:expr)) => {
        $crate::traits::expressive::ExpressiveEnum::Nested({
            #[allow(unused_imports)]
            use $crate::traits::expressive::Expressive;
            $expr.expr()
        })
    };

    ({$deferred:expr}) => {
        $deferred.into()
    };

    ($param:expr) => {
        $crate::traits::expressive::ExpressiveEnum::Scalar($param.into())
    };
}
