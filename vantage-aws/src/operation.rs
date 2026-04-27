//! `column.eq(value)` / `column.in_(subquery)` for AWS-backed tables.
//!
//! Bring [`AwsOperation`] into scope and any column or identifier
//! expression you already have picks up the methods automatically:
//!
//! ```ignore
//! use vantage_aws::AwsOperation;
//!
//! let cond = events["logGroupName"].eq("/aws/lambda/foo");
//! ```
//!
//! For literal multi-value sets — rare, since AWS only accepts a
//! single value anyway — call [`crate::in_`] directly.

use ciborium::Value as CborValue;
use vantage_expressions::Expressive;

use crate::condition::AwsCondition;

fn field_name<T>(expr: &(impl Expressive<T> + ?Sized)) -> String {
    expr.expr().template
}

/// `eq` / `in_` for AWS-backed columns. Auto-implemented for any
/// `Expressive<CborValue>` — `Column<T>`, identifier expressions, etc.
pub trait AwsOperation<T>: Expressive<T> {
    /// `column == value`.
    fn eq(&self, value: impl Into<CborValue>) -> AwsCondition
    where
        Self: Sized,
    {
        AwsCondition::eq(field_name(self), value)
    }

    /// `column == value` where `value` comes from another query.
    /// The subquery runs at execute time and must yield exactly one
    /// value (AWS APIs don't accept multi-value filters).
    ///
    /// Typical call:
    /// `events["logGroupName"].in_(source.column_values_expr("logGroupName"))`.
    fn in_<E>(&self, source: E) -> AwsCondition
    where
        Self: Sized,
        E: Expressive<CborValue>,
    {
        AwsCondition::Deferred {
            field: field_name(self),
            source: source.expr(),
        }
    }
}

impl<T, S: Expressive<T>> AwsOperation<T> for S {}

#[cfg(test)]
mod tests {
    use super::*;
    use vantage_expressions::Expression;
    use vantage_table::column::core::Column;

    #[test]
    fn column_eq_produces_aws_condition() {
        let c = Column::<String>::new("logGroupName");
        match c.eq("/aws/lambda/foo") {
            AwsCondition::Eq { field, value } => {
                assert_eq!(field, "logGroupName");
                assert!(matches!(value, CborValue::Text(ref s) if s == "/aws/lambda/foo"));
            }
            other => panic!("expected Eq, got {other:?}"),
        }
    }

    #[test]
    fn column_in_produces_deferred() {
        let c = Column::<String>::new("logGroupName");
        let source: Expression<CborValue> = Expression::new("subquery", vec![]);
        match c.in_(source) {
            AwsCondition::Deferred { field, source } => {
                assert_eq!(field, "logGroupName");
                assert_eq!(source.template, "subquery");
            }
            other => panic!("expected Deferred, got {other:?}"),
        }
    }
}
