//! CSV implementation of the generic Operation trait.
//!
//! Implements `eq()` and `in_()` for `Column<AnyCsvType>`,
//! which is the column type used by CSV tables (accessed via `table["field"]`).

pub use vantage_table::operation::{OP_EQ, OP_IN};

use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};
use vantage_table::column::core::Column;
use vantage_table::operation::Operation;

use crate::type_system::AnyCsvType;

impl Operation<AnyCsvType> for Column<AnyCsvType> {
    fn eq(&self, value: AnyCsvType) -> Expression<AnyCsvType> {
        Expression::new(
            OP_EQ,
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Scalar(value),
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
