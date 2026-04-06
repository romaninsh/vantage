//! Generic operation trait for building conditions from any `Expressive` type.
//!
//! A blanket impl provides `eq`, `ne`, `gt`, `gte`, `lt`, `lte`, and `in_` for
//! every type that implements `Expressive<T>`. This includes table columns, fields,
//! scalar values, and expressions across all backends.

use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};

/// Trait for building condition expressions.
///
/// Blanket-implemented for all `Expressive<T>` types using standard SQL templates.
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
