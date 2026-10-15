//! `GraphqlOperation` — typed comparison/logical operators that produce
//! `GraphqlCondition`. Blanket-implemented over all `Expressive<T>`, so
//! typed columns get `.eq()`/`.gt()`/`.in_()`/… for free.
//!
//! The field name is the operand's expression template, which is the column
//! name for a typed `Column<T>`. A complex expression lands its rendered
//! template as the field, so keep operands to plain columns.

use serde_json::Value;
use vantage_expressions::Expressive;

use crate::graphql::condition::{FieldCondition, GraphqlCondition, GraphqlOp};
use crate::graphql::types::{AnyGraphqlType, GraphqlType};

fn field_name<T>(expr: &(impl Expressive<T> + ?Sized)) -> String {
    expr.expr().template.clone()
}

fn to_json(value: impl Into<AnyGraphqlType>) -> Value {
    value.into().to_json()
}

fn to_json_array<I, V>(values: I) -> Value
where
    I: IntoIterator<Item = V>,
    V: Into<AnyGraphqlType>,
{
    Value::Array(values.into_iter().map(to_json).collect())
}

/// GraphQL operators that produce a [`GraphqlCondition`].
///
/// Blanket-impl'd for any `Expressive<T>`. Import this trait alongside
/// your column types when writing GraphQL filters; don't mix it with
/// `vantage_table::operation::Operation` in the same scope (which would
/// shadow these methods with raw-expression versions).
pub trait GraphqlOperation<T>: Expressive<T> {
    /// `field = value`
    fn eq(&self, value: impl Into<AnyGraphqlType>) -> GraphqlCondition
    where
        Self: Sized,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::Eq,
            to_json(value),
        ))
    }

    /// `field != value`
    fn ne(&self, value: impl Into<AnyGraphqlType>) -> GraphqlCondition
    where
        Self: Sized,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::Ne,
            to_json(value),
        ))
    }

    /// `field > value`
    fn gt(&self, value: impl Into<AnyGraphqlType>) -> GraphqlCondition
    where
        Self: Sized,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::Gt,
            to_json(value),
        ))
    }

    /// `field >= value`
    fn gte(&self, value: impl Into<AnyGraphqlType>) -> GraphqlCondition
    where
        Self: Sized,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::Gte,
            to_json(value),
        ))
    }

    /// `field < value`
    fn lt(&self, value: impl Into<AnyGraphqlType>) -> GraphqlCondition
    where
        Self: Sized,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::Lt,
            to_json(value),
        ))
    }

    /// `field <= value`
    fn lte(&self, value: impl Into<AnyGraphqlType>) -> GraphqlCondition
    where
        Self: Sized,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::Lte,
            to_json(value),
        ))
    }

    /// `field IN [values...]`
    fn in_<I, V>(&self, values: I) -> GraphqlCondition
    where
        Self: Sized,
        I: IntoIterator<Item = V>,
        V: Into<AnyGraphqlType>,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::In,
            to_json_array(values),
        ))
    }

    /// `field NOT IN [values...]`
    fn not_in<I, V>(&self, values: I) -> GraphqlCondition
    where
        Self: Sized,
        I: IntoIterator<Item = V>,
        V: Into<AnyGraphqlType>,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::NotIn,
            to_json_array(values),
        ))
    }

    /// `field LIKE pattern` — case-sensitive substring match.
    fn like(&self, pattern: impl Into<String>) -> GraphqlCondition
    where
        Self: Sized,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::Like,
            Value::String(pattern.into()),
        ))
    }

    /// `field ILIKE pattern` — case-insensitive substring match.
    fn ilike(&self, pattern: impl Into<String>) -> GraphqlCondition
    where
        Self: Sized,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::ILike,
            Value::String(pattern.into()),
        ))
    }

    /// `field IS NULL`
    fn is_null(&self) -> GraphqlCondition
    where
        Self: Sized,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::IsNull,
            Value::Null,
        ))
    }

    /// `field IS NOT NULL`
    fn is_not_null(&self) -> GraphqlCondition
    where
        Self: Sized,
    {
        GraphqlCondition::Field(FieldCondition::new(
            field_name(self),
            GraphqlOp::IsNotNull,
            Value::Null,
        ))
    }
}

/// Blanket: any `Expressive<T>` gets `GraphqlOperation<T>` for free.
impl<T, S: Expressive<T>> GraphqlOperation<T> for S {}

// Keeps the `GraphqlType` import used: `to_json` needs the trait in scope.
#[allow(dead_code)]
fn _assert_graphql_type_in_scope<T: GraphqlType>(_: &T) {}
