use std::fmt::{Debug, Display};

use vantage_core::util::IntoVec;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// SQL function call: `NAME(arg1, arg2, ...)`.
///
/// The function name is automatically uppercased. Arguments are expressions
/// that get rendered comma-separated inside the parentheses.
///
/// Prefer the [`fx!`] macro for ergonomic construction — it calls `.expr()`
/// on each argument automatically.
///
/// # Examples
///
/// ```ignore
/// // With macro (preferred):
/// fx!("sum", ident("total").dot_of("o"))
/// fx!("coalesce", fx!("sum", ident("total")), 0.0f64)
///
/// // With struct (when you need IntoVec or programmatic args):
/// Fx::new("sum", [ident("total").dot_of("o").expr()])
/// ```
#[derive(Debug, Clone)]
pub struct Fx<T: Debug + Display + Clone> {
    name: String,
    args: Vec<Expression<T>>,
}

impl<T: Debug + Display + Clone> Fx<T> {
    pub fn new(name: impl Into<String>, args: impl IntoVec<Expression<T>>) -> Self {
        Self {
            name: name.into().to_uppercase(),
            args: args.into_vec(),
        }
    }
}

impl<T: Debug + Display + Clone> Expressive<T> for Fx<T> {
    fn expr(&self) -> Expression<T> {
        let args_expr = Expression::from_vec(self.args.clone(), ", ");
        Expression::new(
            format!("{}({{}})", self.name),
            vec![ExpressiveEnum::Nested(args_expr)],
        )
    }
}

impl<T: Debug + Display + Clone> From<Fx<T>> for Expression<T> {
    fn from(fx: Fx<T>) -> Self {
        fx.expr()
    }
}

/// Macro for building SQL function calls with automatic `.expr()` on arguments.
///
/// Each argument has `.expr()` called via the `Expressive` trait, so you can
/// pass `Identifier`, `Column`, `Fx`, scalars, or any `Expressive<T>` directly.
///
/// # Examples
///
/// ```ignore
/// use vantage_sql::primitives::*;
///
/// // Single argument
/// fx!("count", sqlite_expr!("*"))
///
/// // Multiple arguments
/// fx!("coalesce", ident("name"), "unnamed")
///
/// // Nested function calls
/// fx!("round", fx!("avg", ident("price")), 2i64)
/// ```
#[macro_export]
macro_rules! fx {
    ($name:expr, $($arg:expr),+ $(,)?) => {
        $crate::primitives::fx::Fx::new($name, vec![
            $({
                #[allow(unused_imports)]
                use vantage_expressions::Expressive;
                ($arg).expr()
            }),+
        ])
    };
}
