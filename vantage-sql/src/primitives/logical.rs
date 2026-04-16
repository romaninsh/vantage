//! Logical combinators for SQL conditions: `or_()` and `and_()`.
//!
//! These combine two expressions with `OR` / `AND`, wrapping each side
//! in parentheses to preserve precedence.

use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

/// Combines two conditions with `OR`: `(lhs) OR (rhs)`.
///
/// ```ignore
/// use vantage_sql::primitives::*;
///
/// or_(ident("role").eq("admin"), ident("role").eq("superuser"))
/// // => ("role" = 'admin') OR ("role" = 'superuser')
/// ```
pub fn or_<T>(lhs: impl Expressive<T>, rhs: impl Expressive<T>) -> Expression<T> {
    Expression::new(
        "({}) OR ({})",
        vec![
            ExpressiveEnum::Nested(lhs.expr()),
            ExpressiveEnum::Nested(rhs.expr()),
        ],
    )
}

/// Combines two conditions with `AND`: `(lhs) AND (rhs)`.
///
/// Useful when you need explicit grouping — `with_condition()` already
/// combines multiple conditions with `AND`, but `and_()` lets you nest
/// it inside an `or_()`:
///
/// ```ignore
/// use vantage_sql::primitives::*;
///
/// // (price > 100 AND in_stock = 1) OR (featured = 1)
/// or_(
///     and_(ident("price").gt(100), ident("in_stock").eq(true)),
///     ident("featured").eq(true),
/// )
/// ```
pub fn and_<T>(lhs: impl Expressive<T>, rhs: impl Expressive<T>) -> Expression<T> {
    Expression::new(
        "({}) AND ({})",
        vec![
            ExpressiveEnum::Nested(lhs.expr()),
            ExpressiveEnum::Nested(rhs.expr()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_or_preview() {
        let expr: Expression<i32> = or_(
            Expression::new("a = {}", vec![ExpressiveEnum::Scalar(1)]),
            Expression::new("b = {}", vec![ExpressiveEnum::Scalar(2)]),
        );
        assert_eq!(expr.preview(), "(a = 1) OR (b = 2)");
    }

    #[test]
    fn test_and_preview() {
        let expr: Expression<i32> = and_(
            Expression::new("a = {}", vec![ExpressiveEnum::Scalar(1)]),
            Expression::new("b = {}", vec![ExpressiveEnum::Scalar(2)]),
        );
        assert_eq!(expr.preview(), "(a = 1) AND (b = 2)");
    }

    #[test]
    fn test_nested_or_and() {
        let expr: Expression<i32> = or_(
            and_(
                Expression::new("a = {}", vec![ExpressiveEnum::Scalar(1)]),
                Expression::new("b = {}", vec![ExpressiveEnum::Scalar(2)]),
            ),
            Expression::new("c = {}", vec![ExpressiveEnum::Scalar(3)]),
        );
        assert_eq!(expr.preview(), "((a = 1) AND (b = 2)) OR (c = 3)");
    }
}
