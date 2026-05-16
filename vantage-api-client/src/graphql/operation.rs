//! `GraphqlOperation` — typed comparison/logical operators that produce
//! `GraphqlCondition`. Blanket-implemented over all `Expressive<T>`, so
//! typed columns get `.eq()`/`.gt()`/`.in_()`/… for free.
//!
//! ```ignore
//! use vantage_api_client::graphql::operation::GraphqlOperation;
//! use vantage_table::column::core::Column;
//!
//! let mission = Column::<String>::new("mission_name");
//! let cond = mission.eq("FalconSat");
//! // GraphqlCondition::Field { field: "mission_name", op: Eq, value: "FalconSat" }
//! ```
//!
//! The same pattern as `vantage-mongodb`'s `MongoOperation`. Field name
//! is pulled from `self.expr().template`, which works for typed
//! `Column<T>` (the column name comes out verbatim). Complex
//! expressions land their rendered template as the field, which is
//! rarely what you want — keep operands simple.

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

// Tip the type checker about the unused import in builds that don't
// touch the chrono module — `to_json` only needs the `GraphqlType` trait
// in scope through this path.
#[allow(dead_code)]
fn _assert_graphql_type_in_scope<T: GraphqlType>(_: &T) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::condition::FilterDialect;
    use serde_json::json;
    use vantage_table::column::core::Column;

    #[tokio::test]
    async fn column_eq_renders_hasura() {
        let mission = Column::<String>::new("mission_name");
        let cond = mission.eq("FalconSat");
        let r = cond.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(r, json!({ "mission_name": { "_eq": "FalconSat" } }));
    }

    #[tokio::test]
    async fn column_eq_renders_generic() {
        let mission = Column::<String>::new("mission_name");
        let cond = mission.eq("FalconSat");
        let r = cond.render(FilterDialect::Generic).await.unwrap();
        assert_eq!(r, json!({ "mission_name": "FalconSat" }));
    }

    #[tokio::test]
    async fn column_gt_renders_hasura() {
        let price = Column::<i64>::new("price");
        let cond = price.gt(100i64);
        let r = cond.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(r, json!({ "price": { "_gt": 100 } }));
    }

    #[tokio::test]
    async fn column_in_renders_hasura_array() {
        let status = Column::<String>::new("status");
        let cond = status.in_(vec!["active", "pending"]);
        let r = cond.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(r, json!({ "status": { "_in": ["active", "pending"] } }));
    }

    #[tokio::test]
    async fn column_is_null_renders_hasura_bool() {
        let deleted = Column::<String>::new("deleted_at");
        let cond = deleted.is_null();
        let r = cond.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(r, json!({ "deleted_at": { "_is_null": true } }));
    }

    #[tokio::test]
    async fn column_ilike_renders_hasura() {
        let name = Column::<String>::new("name");
        let cond = name.ilike("%falcon%");
        let r = cond.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(r, json!({ "name": { "_ilike": "%falcon%" } }));
    }
}
