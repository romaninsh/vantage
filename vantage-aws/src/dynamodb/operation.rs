//! DynamoDB operation trait — typed `.eq()`/`.gt()`/… methods producing
//! `DynamoCondition`. Blanket-implemented over `Expressive<T>` so columns
//! pick up these methods for free.
//!
//! v0 only wires `.eq()`. The richer set (`.gt`, `.between`, `.in_`,
//! `.begins_with`) lands alongside `Scan`/`Query` filter execution.

use vantage_expressions::Expressive;

use super::condition::DynamoCondition;
use super::types::AnyDynamoType;

fn field_name<T>(expr: &(impl Expressive<T> + ?Sized)) -> String {
    expr.expr().template
}

pub trait DynamoOperation<T>: Expressive<T> {
    /// `field = :value` — see `DynamoCondition::eq`.
    fn eq(&self, value: impl Into<AnyDynamoType>) -> DynamoCondition
    where
        Self: Sized,
    {
        let any: AnyDynamoType = value.into();
        DynamoCondition::eq(field_name(self), any.into_value())
    }
}

impl<T, S: Expressive<T>> DynamoOperation<T> for S {}
