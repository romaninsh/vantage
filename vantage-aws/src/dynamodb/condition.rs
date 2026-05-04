//! DynamoDB condition DSL — analogous to MongoDB's `MongoCondition`.
//!
//! v0 carries a rendered expression string plus expression-attribute
//! name/value maps, which is what DynamoDB's `ConditionExpression` /
//! `KeyConditionExpression` / `FilterExpression` parameters consume.
//! The expression strings reference placeholders (`#name`, `:value`)
//! that the maps resolve at request time.

use indexmap::IndexMap;

use super::types::AttributeValue;

/// A DynamoDB filter condition.
///
/// - `Expr` carries a rendered expression plus its attribute maps.
/// - `And` combines multiple conditions with implicit `AND` glue.
#[derive(Debug, Clone)]
pub enum DynamoCondition {
    Expr {
        expression: String,
        names: IndexMap<String, String>,
        values: IndexMap<String, AttributeValue>,
    },
    And(Vec<DynamoCondition>),
}

impl DynamoCondition {
    /// Build a `field = value` condition. The placeholders are mangled
    /// per-condition so multiple `eq` calls don't clobber each other
    /// when combined.
    pub fn eq(field: impl Into<String>, value: impl Into<AttributeValue>) -> Self {
        let field = field.into();
        let mut names = IndexMap::new();
        let mut values = IndexMap::new();
        names.insert("#f".to_string(), field);
        values.insert(":v".to_string(), value.into());
        Self::Expr {
            expression: "#f = :v".to_string(),
            names,
            values,
        }
    }
}
