//! Generic operation trait for building conditions on table columns.
//!
//! Each persistence backend provides its own implementation.
//! CSV uses structured parameters evaluated in memory.
//! SQL/SurrealDB render as query syntax.

use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};

/// Trait for building condition expressions from column references.
///
/// Provides default implementations using standard SQL syntax.
/// Backends like CSV override these with structured parameters for in-memory evaluation.
pub trait Operation<T>: Expressive<T> {
    /// Creates an equality condition: field = value
    fn eq(&self, value: impl Into<T>) -> Expression<T> {
        Expression::new(
            "{} = {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Scalar(value.into()),
            ],
        )
    }

    /// Creates a membership condition: field IN (values_expression)
    fn in_(&self, values: Expression<T>) -> Expression<T> {
        Expression::new(
            "{} IN ({})",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(values),
            ],
        )
    }
}
