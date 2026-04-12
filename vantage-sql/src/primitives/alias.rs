use super::identifier::Identifier;
use vantage_expressions::{expr_any, Expression, Expressive};

/// Extension trait that adds `.as_alias()` to any [`Expressive<T>`] type.
///
/// Wraps the expression as `(expr) AS <quoted_alias>`, using [`Identifier`]
/// for backend-aware quoting.
///
/// ```ignore
/// use vantage_sql::primitives::alias::AliasExt;
///
/// Fx::new("count", [mysql_expr!("*")]).as_alias("cnt")
/// // → (COUNT(*)) AS `cnt`   (MySQL)
/// // → (COUNT(*)) AS "cnt"   (PostgreSQL)
/// ```
pub trait AliasExt<T>: Expressive<T> + Sized {
    fn as_alias(self, alias: impl Into<String>) -> Expression<T>
    where
        Identifier: Expressive<T>,
    {
        expr_any!("{} AS {}", (self), (Identifier::new(alias)))
    }
}

impl<T, E: Expressive<T> + Sized> AliasExt<T> for E {}
