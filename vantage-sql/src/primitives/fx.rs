use std::fmt::{Debug, Display};

use vantage_core::util::IntoVec;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// SQL function call: `NAME(arg1, arg2, ...)`.
///
/// The function name is automatically uppercased. Arguments are expressions
/// that get rendered comma-separated inside the parentheses.
///
/// # Examples
///
/// ```ignore
/// use vantage_sql::primitives::fx::Fx;
///
/// // Single arg: SUM("o"."total")
/// Fx::new("sum", [ident("total").dot_of("o").expr()])
///
/// // Multiple args: COALESCE(SUM("o"."total"), 0.0)
/// Fx::new("coalesce", [
///     Fx::new("sum", [ident("total").dot_of("o").expr()]).expr(),
///     sqlite_expr!("{}", 0.0f64),
/// ])
/// ```
#[derive(Debug, Clone)]
pub struct Fx<T: Debug + Display + Clone> {
    name: String,
    args: Vec<Expression<T>>,
    alias: Option<String>,
}

impl<T: Debug + Display + Clone> Fx<T> {
    pub fn new(name: impl Into<String>, args: impl IntoVec<Expression<T>>) -> Self {
        Self {
            name: name.into().to_uppercase(),
            args: args.into_vec(),
            alias: None,
        }
    }

    pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
        self.alias = Some(alias.into());
        self
    }
}

impl<T: Debug + Display + Clone> Expressive<T> for Fx<T> {
    fn expr(&self) -> Expression<T> {
        let args_expr = Expression::from_vec(self.args.clone(), ", ");
        let base = Expression::new(
            format!("{}({{}})", self.name),
            vec![ExpressiveEnum::Nested(args_expr)],
        );
        match &self.alias {
            Some(alias) => Expression::new(
                format!("{{}} AS \"{}\"", alias),
                vec![ExpressiveEnum::Nested(base)],
            ),
            None => base,
        }
    }
}

impl<T: Debug + Display + Clone> From<Fx<T>> for Expression<T> {
    fn from(fx: Fx<T>) -> Self {
        fx.expr()
    }
}
