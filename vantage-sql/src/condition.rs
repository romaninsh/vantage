//! Backend-specific condition wrappers for type-safe `Column<T>` operations.
//!
//! Each wrapper (e.g., `SqliteCondition`) is a newtype around `Expression<BackendType>`.
//! It accepts `Expression<BackendType>` and common types (`Identifier`, `Fx`) via `From`.
//!
//! For typed column operations (`Column::<i64>::new("price").gt(150)` which returns
//! `Expression<i64>`), use `with_typed_condition()` on the Selectable trait, which
//! maps the expression type before wrapping.

use vantage_expressions::Expression;

use crate::primitives::fx::Fx;
use crate::primitives::identifier::Identifier;

macro_rules! define_sql_condition {
    ($name:ident, $any_type:ty) => {
        /// Condition wrapper that preserves type inference for `with_condition()`.
        #[derive(Debug, Clone)]
        pub struct $name(pub Expression<$any_type>);

        impl $name {
            pub fn into_expr(self) -> Expression<$any_type> {
                self.0
            }

            /// Create from a typed expression by mapping scalars via `Into<BackendType>`.
            ///
            /// Used by `with_typed_condition()` to accept `Expression<i64>`, etc.
            pub fn from_typed<F>(expr: Expression<F>) -> Self
            where
                F: Into<$any_type> + Send + Clone + 'static,
            {
                use vantage_expressions::ExpressionMap;
                Self(expr.map())
            }
        }

        // From Expression<BackendType> — identity, preserves inference
        impl From<Expression<$any_type>> for $name {
            fn from(expr: Expression<$any_type>) -> Self {
                Self(expr)
            }
        }

        // From Identifier
        impl From<Identifier> for $name {
            fn from(id: Identifier) -> Self {
                use vantage_expressions::Expressive;
                Self(id.expr())
            }
        }

        // From Fx<BackendType>
        impl From<Fx<$any_type>> for $name {
            fn from(fx: Fx<$any_type>) -> Self {
                Self(fx.into())
            }
        }
    };
}

#[cfg(feature = "sqlite")]
define_sql_condition!(SqliteCondition, crate::sqlite::types::AnySqliteType);

#[cfg(feature = "postgres")]
define_sql_condition!(PostgresCondition, crate::postgres::types::AnyPostgresType);

#[cfg(feature = "mysql")]
define_sql_condition!(MysqlCondition, crate::mysql::types::AnyMysqlType);

// MySQL-specific: FulltextMatch
#[cfg(feature = "mysql")]
impl From<crate::mysql::statements::primitives::FulltextMatch> for MysqlCondition {
    fn from(fm: crate::mysql::statements::primitives::FulltextMatch) -> Self {
        Self(fm.into())
    }
}
