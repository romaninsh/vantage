use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use super::types::AnyMysqlType;

type Expr = Expression<AnyMysqlType>;

/// MySQL-specific operations on expressions.
pub trait MysqlOperation: Expressive<AnyMysqlType> {
    /// `expr REGEXP pattern`
    fn regexp(&self, pattern: impl Expressive<AnyMysqlType>) -> Expr {
        Expression::new(
            "{} REGEXP {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(pattern.expr()),
            ],
        )
    }

    /// `expr & other` (bitwise AND)
    fn bitand(&self, other: impl Expressive<AnyMysqlType>) -> Expr {
        Expression::new(
            "{} & {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(other.expr()),
            ],
        )
    }

    /// `expr | other` (bitwise OR)
    fn bitor(&self, other: impl Expressive<AnyMysqlType>) -> Expr {
        Expression::new(
            "{} | {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(other.expr()),
            ],
        )
    }
}

impl<T: Expressive<AnyMysqlType>> MysqlOperation for T {}
