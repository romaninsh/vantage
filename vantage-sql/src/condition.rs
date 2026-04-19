//! Backend-specific condition wrappers and operation traits.
//!
//! **Conditions:** Each wrapper (e.g., `SqliteCondition`) is a newtype around
//! `Expression<BackendType>`. It accepts `Expression<F>` for any `F: Into<BackendType>`,
//! plus common types (`Identifier`, `Fx`) via `From`.
//!
//! **Operations:** Each backend gets a vendor-specific operation trait (e.g.
//! `SqliteOperation<T>`) that produces the backend's condition type directly.
//! These are blanket-implemented for all `Expressive<T>` where `T: Into<AnyType>`,
//! and the condition type implements `Expressive<AnyType>` to enable chaining:
//!
//! ```ignore
//! use vantage_sql::sqlite::operation::SqliteOperation;
//! let price = Column::<i64>::new("price");
//! price.gt(10).eq(false)  // => SqliteCondition wrapping (price > 10) = 0
//! ```

use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};

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
            /// Used by the generic `From<Expression<F>>` impl.
            pub fn from_typed<F>(expr: Expression<F>) -> Self
            where
                F: Into<$any_type> + Send + Clone + 'static,
            {
                use vantage_expressions::ExpressionMap;
                Self(expr.map())
            }
        }

        // From Expression<F> where F: Into<BackendType> — accepts both
        // Expression<BackendType> (identity) and typed Expression<i64> etc.
        impl<F> From<Expression<F>> for $name
        where
            F: Into<$any_type> + Send + Clone + 'static,
        {
            fn from(expr: Expression<F>) -> Self {
                Self::from_typed(expr)
            }
        }

        // From Identifier
        impl From<Identifier> for $name {
            fn from(id: Identifier) -> Self {
                use vantage_expressions::Expressive;
                Self(id.expr())
            }
        }

        // Into Expression<BackendType> — unwrap the newtype
        impl From<$name> for Expression<$any_type> {
            fn from(cond: $name) -> Self {
                cond.0
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

// ── Backend-typed identifier wrapper ────────────────────────────────

/// Defines a backend-specific identifier wrapper that only implements
/// `Expressive<$any_type>`, avoiding ambiguity when multiple backend
/// features are enabled.
///
/// Usage: `define_typed_ident!(PgIdent, pg_ident, AnyPostgresType, PostgresCondition);`
#[macro_export]
macro_rules! define_typed_ident {
    ($struct_name:ident, $fn_name:ident, $any_type:ty, $condition:ty) => {
        #[derive(Debug, Clone)]
        pub struct $struct_name($crate::primitives::identifier::Identifier);

        impl $struct_name {
            pub fn new(name: impl Into<String>) -> Self {
                Self($crate::primitives::identifier::ident(name))
            }

            pub fn dot_of(mut self, prefix: impl Into<String>) -> Self {
                self.0 = self.0.dot_of(prefix);
                self
            }

            pub fn with_alias(mut self, alias: impl Into<String>) -> Self {
                self.0 = self.0.with_alias(alias);
                self
            }
        }

        impl $crate::vantage_expressions::Expressive<$any_type> for $struct_name {
            fn expr(&self) -> $crate::vantage_expressions::Expression<$any_type> {
                $crate::vantage_expressions::Expressive::<$any_type>::expr(&self.0)
            }
        }

        impl From<$struct_name> for $crate::vantage_expressions::Expression<$any_type> {
            fn from(id: $struct_name) -> Self {
                $crate::vantage_expressions::Expressive::<$any_type>::expr(&id.0)
            }
        }

        impl From<$struct_name> for $condition {
            fn from(id: $struct_name) -> Self {
                Self::from_typed($crate::vantage_expressions::Expressive::<$any_type>::expr(
                    &id.0,
                ))
            }
        }

        /// Shorthand constructor.
        pub fn $fn_name(name: impl Into<String>) -> $struct_name {
            $struct_name::new(name)
        }
    };
}

// ── Vendor-specific operation traits ─────────────────────────────────

#[macro_export]
macro_rules! define_sql_operation {
    ($trait_name:ident, $condition:ident, $any_type:ty) => {
        /// Vendor-specific operations producing the backend's condition type.
        ///
        /// Blanket-implemented for all `Expressive<T>` where `T: Into<AnyType>`.
        /// The condition type itself implements `Expressive<AnyType>`, enabling
        /// cross-type chaining like `price.gt(10).eq(false)`.
        pub trait $trait_name<T>: $crate::vantage_expressions::Expressive<T>
        where
            T: Into<$any_type> + Send + Clone + 'static,
        {
            /// `field = value`
            fn eq(&self, value: impl $crate::vantage_expressions::Expressive<T>) -> $condition
            where
                Self: Sized,
            {
                $crate::condition::build_sql_binary::<T, $any_type, $condition>(
                    self, value, "{} = {}",
                )
            }

            /// `field != value`
            fn ne(&self, value: impl $crate::vantage_expressions::Expressive<T>) -> $condition
            where
                Self: Sized,
            {
                $crate::condition::build_sql_binary::<T, $any_type, $condition>(
                    self, value, "{} != {}",
                )
            }

            /// `field > value`
            fn gt(&self, value: impl $crate::vantage_expressions::Expressive<T>) -> $condition
            where
                Self: Sized,
            {
                $crate::condition::build_sql_binary::<T, $any_type, $condition>(
                    self, value, "{} > {}",
                )
            }

            /// `field >= value`
            fn gte(&self, value: impl $crate::vantage_expressions::Expressive<T>) -> $condition
            where
                Self: Sized,
            {
                $crate::condition::build_sql_binary::<T, $any_type, $condition>(
                    self, value, "{} >= {}",
                )
            }

            /// `field < value`
            fn lt(&self, value: impl $crate::vantage_expressions::Expressive<T>) -> $condition
            where
                Self: Sized,
            {
                $crate::condition::build_sql_binary::<T, $any_type, $condition>(
                    self, value, "{} < {}",
                )
            }

            /// `field <= value`
            fn lte(&self, value: impl $crate::vantage_expressions::Expressive<T>) -> $condition
            where
                Self: Sized,
            {
                $crate::condition::build_sql_binary::<T, $any_type, $condition>(
                    self, value, "{} <= {}",
                )
            }

            /// `field IN (values_expression)`
            fn in_(&self, values: impl $crate::vantage_expressions::Expressive<T>) -> $condition
            where
                Self: Sized,
            {
                $crate::condition::build_sql_binary::<T, $any_type, $condition>(
                    self,
                    values,
                    "{} IN ({})",
                )
            }

            /// `field IN (a, b, c)` from a slice of scalar values
            fn in_list<V: Into<T> + Clone>(&self, values: &[V]) -> $condition
            where
                Self: Sized,
                T: Clone,
            {
                use $crate::vantage_expressions::Expression;
                use $crate::vantage_expressions::traits::expressive::ExpressiveEnum;
                let params: Vec<Expression<T>> = values
                    .iter()
                    .map(|v| Expression::new("{}", vec![ExpressiveEnum::Scalar(v.clone().into())]))
                    .collect();
                let expr: Expression<T> = Expression::new(
                    "{} IN ({})",
                    vec![
                        ExpressiveEnum::Nested(self.expr()),
                        ExpressiveEnum::Nested(Expression::from_vec(params, ", ")),
                    ],
                );
                $condition::from_typed(expr)
            }

            /// `CAST(expr AS type_name)`
            fn cast(&self, type_name: &str) -> $condition
            where
                Self: Sized,
            {
                use $crate::vantage_expressions::Expression;
                use $crate::vantage_expressions::traits::expressive::ExpressiveEnum;
                let expr: Expression<T> = Expression::new(
                    format!("CAST({{}} AS {type_name})"),
                    vec![ExpressiveEnum::Nested(self.expr())],
                );
                $condition::from_typed(expr)
            }

            /// `field IS NULL`
            fn is_null(&self) -> $condition
            where
                Self: Sized,
            {
                use $crate::vantage_expressions::Expression;
                use $crate::vantage_expressions::traits::expressive::ExpressiveEnum;
                let expr: Expression<T> =
                    Expression::new("{} IS NULL", vec![ExpressiveEnum::Nested(self.expr())]);
                $condition::from_typed(expr)
            }

            /// `field IS NOT NULL`
            fn is_not_null(&self) -> $condition
            where
                Self: Sized,
            {
                use $crate::vantage_expressions::Expression;
                use $crate::vantage_expressions::traits::expressive::ExpressiveEnum;
                let expr: Expression<T> =
                    Expression::new("{} IS NOT NULL", vec![ExpressiveEnum::Nested(self.expr())]);
                $condition::from_typed(expr)
            }
        }

        /// Blanket: any `Expressive<T>` where `T: Into<AnyType>` gets the
        /// operation trait for free.
        impl<T, S> $trait_name<T> for S
        where
            S: $crate::vantage_expressions::Expressive<T>,
            T: Into<$any_type> + Send + Clone + 'static,
        {
        }

        /// Condition chaining: the condition type wraps `Expression<AnyType>`,
        /// so implementing `Expressive<AnyType>` gives it the operation trait
        /// via the blanket above.
        impl $crate::vantage_expressions::Expressive<$any_type> for $condition {
            fn expr(&self) -> $crate::vantage_expressions::Expression<$any_type> {
                self.0.clone()
            }
        }
    };
}

/// Helper for `define_sql_operation!`: build a binary expression, map to
/// the backend's condition type. Public so the macro can call it from
/// any module.
pub fn build_sql_binary<T, AnyType, Cond>(
    lhs: &(impl Expressive<T> + ?Sized),
    rhs: impl Expressive<T>,
    template: &str,
) -> Cond
where
    T: Into<AnyType> + Send + Clone + 'static,
    Cond: From<Expression<T>>,
{
    let expr: Expression<T> = Expression::new(
        template,
        vec![
            ExpressiveEnum::Nested(lhs.expr()),
            ExpressiveEnum::Nested(rhs.expr()),
        ],
    );
    Cond::from(expr)
}
