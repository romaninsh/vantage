//! CSV operations for expressions
//!
//! Provides `eq()` and `in_()` operations for CSV columns, analogous to
//! SurrealDB's `RefOperation`. Instead of building query template strings,
//! these produce structured `Expression<AnyCsvType>` that CSV's fetch methods
//! can peel apart and evaluate in memory.

use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};

use crate::type_system::{AnyCsvType, CsvType};

/// Expression template markers used by CSV condition evaluation.
/// These are not parsed as a language — they're just identifiers so
/// `apply_condition` knows which operation to perform.
pub const OP_EQ: &str = "{} = {}";
pub const OP_IN: &str = "{} IN ({})";

/// Extension trait providing comparison operations for CSV expressions.
///
/// Blanket-implemented for anything that implements `Expressive<AnyCsvType>`,
/// which includes `Column<AnyCsvType>` (accessed via `table["field"]`).
///
/// # Example
///
/// ```rust,ignore
/// use vantage_csv::CsvOperation;
///
/// let mut table = Client::csv_table(csv);
/// table.add_condition(table["is_paying_client"].eq(true));
/// ```
pub trait CsvOperation: Expressive<AnyCsvType> {
    /// Creates an equality condition: field = value
    ///
    /// The resulting expression has:
    /// - param[0]: Nested(field expression) — the field name
    /// - param[1]: Scalar(value) — the expected value
    fn eq(&self, value: impl CsvType) -> Expression<AnyCsvType>;

    /// Creates a membership condition: field IN (values)
    ///
    /// The resulting expression has:
    /// - param[0]: Nested(field expression) — the field name
    /// - param[1]: Nested/Deferred — resolves to a list of values to match against
    fn in_(&self, values: ExpressiveEnum<AnyCsvType>) -> Expression<AnyCsvType>;
}

impl<T> CsvOperation for T
where
    T: Expressive<AnyCsvType>,
{
    fn eq(&self, value: impl CsvType) -> Expression<AnyCsvType> {
        Expression::new(
            OP_EQ,
            vec![
                ExpressiveEnum::Nested(self.expr()),
                ExpressiveEnum::Scalar(AnyCsvType::new(value)),
            ],
        )
    }

    fn in_(&self, values: ExpressiveEnum<AnyCsvType>) -> Expression<AnyCsvType> {
        Expression::new(OP_IN, vec![ExpressiveEnum::Nested(self.expr()), values])
    }
}
