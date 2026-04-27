//! AWS-specific operation trait that produces `AwsCondition` from columns.
//!
//! Mirrors `vantage-redb`'s `RedbOperation` shape — blanket-implemented
//! for any `Expressive<CborValue>` so `Column<T>` and bare identifiers
//! can write `column.eq(value)` / `column.in_(expr)` and get
//! `AwsCondition` back, ready for `Table::add_condition`.
//!
//! Why a custom trait when `vantage-table` already has a generic
//! `Operation`? Because the generic one returns `Expression<T>`; AWS's
//! condition type is `AwsCondition` (a small enum), so we need our own
//! producer to keep the call-site ergonomics.
//!
//! `in_` takes an `Expressive<CborValue>` (typically the deferred
//! subquery from `Table::column_values_expr`) and produces
//! `AwsCondition::Deferred`. For literal multi-value sets, use the
//! free-function constructor [`crate::in_`] directly.

use ciborium::Value as CborValue;
use vantage_expressions::Expressive;

use crate::condition::AwsCondition;

fn field_name<T>(expr: &(impl Expressive<T> + ?Sized)) -> String {
    expr.expr().template
}

/// AWS conditions on `Column<T>` and other `Expressive<CborValue>` values.
pub trait AwsOperation<T>: Expressive<T> {
    /// `column == value`
    fn eq(&self, value: impl Into<CborValue>) -> AwsCondition
    where
        Self: Sized,
    {
        AwsCondition::eq(field_name(self), value)
    }

    /// `column IN <expression>` — the expression resolves
    /// asynchronously at execute time. AWS APIs only accept exact
    /// matches, so the resolved value list must contain exactly one
    /// element; multi-value resolution errors at query time.
    ///
    /// Most natural call: `events["logGroupName"].in_(source.column_values_expr("logGroupName"))`.
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
