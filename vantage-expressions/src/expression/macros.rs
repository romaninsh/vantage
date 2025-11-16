/// Macro to create expressions with template and parameters for any type T
/// Syntax:
/// - expr_as!(T, "template", arg) -> arg becomes Scalar
/// - expr_as!(T, "template", (expr)) -> expr becomes Nested
/// - expr_as!(T, "template", {deferred}) -> deferred becomes Deferred
#[macro_export]
macro_rules! expr_as {
    // Simple template without parameters: expr_as!(T, "age")
    ($t:ty, $template:expr) => {
        $crate::expression::expression::Expression::<$t>::new($template, vec![])
    };

    // Template with parameters
    ($t:ty, $template:expr, $($param:tt),*) => {
        $crate::expression::expression::Expression::<$t>::new(
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
#[macro_export]
macro_rules! expr_any {
    // Simple template without parameters: expr_any!("age")
    ($template:expr) => {
        $crate::expression::expression::Expression::new($template, vec![])
    };

    // Template with parameters
    ($template:expr, $($param:tt),*) => {
        $crate::expression::expression::Expression::new(
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
        $crate::expr_as!(serde_json::Value, $template)
    };

    // Template with parameters
    ($template:expr, $($param:tt),*) => {
        $crate::expr_as!(serde_json::Value, $template, $($param),*)
    };
}

/// Helper macro to handle different parameter syntaxes
#[macro_export]
macro_rules! expr_param {
    // Nested expression: (expr) -> ExpressiveEnum::Nested(expr)
    // If expr implements Expressive, convert it to Expression first
    (($expr:expr)) => {
        $crate::traits::expressive::ExpressiveEnum::Nested({
            use $crate::traits::expressive::Expressive;
            $expr.expr()
        })
    };

    // Deferred function: {fn} -> ExpressiveEnum::Deferred(fn)
    ({$deferred:expr}) => {
        $deferred.into()
    };

    // Regular scalar: expr -> ExpressiveEnum::Scalar(expr.into())
    ($param:expr) => {
        $crate::traits::expressive::ExpressiveEnum::Scalar($param.into())
    };
}

#[cfg(test)]
mod tests {
    use crate::Expression;

    #[test]
    fn test_expr_macro() {
        let expr = expr!("age > {}", 18);
        assert_eq!(expr.template, "age > {}");
        assert_eq!(expr.parameters.len(), 1);
    }

    #[test]
    fn test_expr_as_macro() {
        let expr = expr_as!(i32, "age > {}", 18);
        assert_eq!(expr.template, "age > {}");
        assert_eq!(expr.parameters.len(), 1);
    }

    #[test]
    fn test_expr_any_macro() {
        let expr: Expression<i16> = expr_any!("age > {}", 18i16);
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
        let expr = expr_as!(String, "Hello {}", "world");
        assert_eq!(expr.preview(), "Hello world");
    }
}
