/// Build a SurrealDB expression; each `{}` in the template takes one argument:
/// - `surreal_expr!("template")`: no parameters;
/// - `surreal_expr!("template", scalar)`: a bound scalar, any
///   `Into<AnySurrealType>` (`i8`–`u64`, `isize`/`usize`, `f32`/`f64`, `bool`,
///   `String`, `&str`);
/// - `surreal_expr!("template", (expr))`: a nested expression;
/// - `surreal_expr!("template", {deferred})`: a deferred value.
///
/// Each argument is a single token, so bind a computed scalar to a local first.
#[macro_export]
macro_rules! surreal_expr {
    ($template:expr) => {
        vantage_expressions::Expression::<$crate::AnySurrealType>::new($template, vec![])
    };

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
