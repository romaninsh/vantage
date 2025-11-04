//! Owned expressions will greedily own all the parameters.
//! Owned expressions implement Expressive trait

use crate::protocol::expressive::{Expressive, ExpressiveEnum};

/// Owned expression contains template and Vec of IntoExpressive parameters
#[derive(Clone)]
pub struct Expression<T> {
    pub template: String,
    pub parameters: Vec<ExpressiveEnum<T>>,
}

impl<T: Clone> Expressive<T> for Expression<T> {
    fn expr(&self) -> Expression<T> {
        self.clone()
    }
}

impl<T> From<Expression<T>> for ExpressiveEnum<T> {
    fn from(expr: Expression<T>) -> Self {
        ExpressiveEnum::Nested(expr)
    }
}

impl<T: std::fmt::Debug + std::fmt::Display> std::fmt::Debug for Expression<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.preview())
    }
}

/// Macro to create expressions with template and parameters for any type T
/// Syntax:
/// - expr_any!(T, "template", arg) -> arg becomes Scalar
/// - expr_any!(T, "template", (expr)) -> expr becomes Nested
/// - expr_any!(T, "template", {deferred}) -> deferred becomes Deferred
#[macro_export]
macro_rules! expr_any {
    // Simple template without parameters: expr_any!(T, "age")
    ($t:ty, $template:expr) => {
        $crate::expression::owned::Expression::<$t>::new($template, vec![])
    };

    // Template with parameters
    ($t:ty, $template:expr, $($param:tt),*) => {
        $crate::expression::owned::Expression::<$t>::new(
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
    // Simple template without parameters: expr!("age")
    ($template:expr) => {
        $crate::expr_any!(serde_json::Value, $template)
    };

    // Template with parameters
    ($template:expr, $($param:tt),*) => {
        $crate::expr_any!(serde_json::Value, $template, $($param),*)
    };
}

/// Helper macro to handle different parameter syntaxes
#[macro_export]
macro_rules! expr_param {
    // Nested expression: (expr) -> ExpressiveEnum::Nested(expr)
    (($expr:expr)) => {
        $crate::protocol::expressive::ExpressiveEnum::Nested($expr)
    };

    // Deferred function: {fn} -> ExpressiveEnum::Deferred(fn)
    ({$deferred:expr}) => {
        $deferred.into()
    };

    // Regular scalar: expr -> ExpressiveEnum::Scalar(expr.into())
    ($param:expr) => {
        $crate::protocol::expressive::ExpressiveEnum::Scalar($param.into())
    };
}

impl<T> Expression<T> {
    /// Create a new owned expression with template and parameters
    pub fn new(template: impl Into<String>, parameters: Vec<ExpressiveEnum<T>>) -> Self {
        Self {
            template: template.into(),
            parameters,
        }
    }

    /// Create expression from vector of expressions and a delimiter
    pub fn from_vec(vec: Vec<Expression<T>>, delimiter: &str) -> Self {
        let template = vec
            .iter()
            .map(|_| "{}")
            .collect::<Vec<&str>>()
            .join(delimiter);

        let parameters = vec.into_iter().map(ExpressiveEnum::nested).collect();

        Self {
            template,
            parameters,
        }
    }
}

impl<T: std::fmt::Display + std::fmt::Debug> Expression<T> {
    pub fn preview(&self) -> String {
        let mut preview = self.template.clone();
        for param in &self.parameters {
            let param_str = param.preview();
            preview = preview.replacen("{}", &param_str, 1);
        }
        preview
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_expr_macro() {
        let expr = expr!("age > {}", 18);
        assert_eq!(expr.template, "age > {}");
        assert_eq!(expr.parameters.len(), 1);
    }

    #[test]
    fn test_expr_any_macro() {
        let expr = expr_any!(i32, "age > {}", 18);
        assert_eq!(expr.template, "age > {}");
        assert_eq!(expr.parameters.len(), 1);
    }

    #[test]
    fn test_nested_expr() {
        let inner = expr!("status = {}", "active");
        let outer = expr!("WHERE {} AND age > {}", (inner), "21");

        assert_eq!(outer.template, "WHERE {} AND age > {}");
        assert_eq!(outer.parameters.len(), 2);
    }

    #[test]
    fn test_preview() {
        let expr = expr_any!(String, "Hello {}", "world");
        assert_eq!(expr.preview(), "Hello world");
    }
}
