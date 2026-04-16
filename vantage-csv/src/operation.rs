//! CSV operation trait and condition operation constants.
//!
//! Template markers for condition operations. CSV's in-memory evaluator
//! matches on these to know which operation to apply.

use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};

use crate::type_system::AnyCsvType;

pub const OP_EQ: &str = "{} = {}";
pub const OP_IN: &str = "{} IN ({})";

/// CSV-specific comparison operations.
///
/// Blanket-implemented for all `Expressive<AnyCsvType>` so columns, fields,
/// and expressions all get `eq`, `in_`, etc. for free.
pub trait CsvOperation: Expressive<AnyCsvType> {
    /// `field = value`
    fn eq(&self, value: impl Expressive<AnyCsvType>) -> Expression<AnyCsvType>
    where
        Self: Sized,
    {
        Expression::new(
            OP_EQ,
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(value.expr()),
            ],
        )
    }

    /// `field IN (values_expression)`
    fn in_(&self, values: impl Expressive<AnyCsvType>) -> Expression<AnyCsvType>
    where
        Self: Sized,
    {
        Expression::new(
            OP_IN,
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(values.expr()),
            ],
        )
    }
}

impl<S: Expressive<AnyCsvType>> CsvOperation for S {}
