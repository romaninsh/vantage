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
    fn eq(&self, value: impl Expressive<T>) -> Expression<T> {
        Expression::new(
            "{} = {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// Creates a not-equal condition: field != value
    fn ne(&self, value: impl Expressive<T>) -> Expression<T> {
        Expression::new(
            "{} != {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// Creates a greater-than condition: field > value
    fn gt(&self, value: impl Expressive<T>) -> Expression<T> {
        Expression::new(
            "{} > {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// Creates a greater-than-or-equal condition: field >= value
    fn gte(&self, value: impl Expressive<T>) -> Expression<T> {
        Expression::new(
            "{} >= {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// Creates a less-than condition: field < value
    fn lt(&self, value: impl Expressive<T>) -> Expression<T> {
        Expression::new(
            "{} < {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// Creates a less-than-or-equal condition: field <= value
    fn lte(&self, value: impl Expressive<T>) -> Expression<T> {
        Expression::new(
            "{} <= {}",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// Creates a membership condition: field IN (values_expression)
    fn in_(&self, values: impl Expressive<T>) -> Expression<T> {
        Expression::new(
            "{} IN ({})",
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(values.expr()),
            ],
        )
    }
}

/// Blanket implementation: any type that implements `Expressive<T>` gets `Operation<T>` for free.
impl<T, S: Expressive<T>> Operation<T> for S {}
