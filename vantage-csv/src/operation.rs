//! CSV implementation of the generic Operation trait.
//!
//! Implements `eq()` and `in_()` for `Column<AnyCsvType>`,
//! which is the column type used by CSV tables (accessed via `table["field"]`).

/// Template markers for condition operations.
/// CSV's in-memory evaluator matches on these to know which operation to apply.
pub const OP_EQ: &str = "{} = {}";
pub const OP_IN: &str = "{} IN ({})";

use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};
use vantage_table::column::core::Column;
use vantage_table::operation::Operation;

use crate::type_system::AnyCsvType;

impl Operation<AnyCsvType> for Column<AnyCsvType> {
    fn eq(&self, value: impl Into<AnyCsvType>) -> Expression<AnyCsvType> {
        Expression::new(
            OP_EQ,
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Scalar(value.into()),
            ],
        )
    }

    fn in_(&self, values: Expression<AnyCsvType>) -> Expression<AnyCsvType> {
        Expression::new(
            OP_IN,
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Nested(values),
            ],
        )
    }
}
