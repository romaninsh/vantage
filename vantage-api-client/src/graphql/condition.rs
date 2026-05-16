//! `GraphqlCondition` — the structured filter type for the GraphQL adapter.
//!
//! Conditions are kept abstract (field/op/value) and rendered to JSON at
//! request time by the dialect attached to the data source. The two
//! dialects that ship today:
//!
//! * [`FilterDialect::Hasura`] — `{ field: { _eq: v }, _and: [...], _or:
//!   [...], _not: {...} }`. Full operator coverage.
//! * [`FilterDialect::Generic`] — flat argument object: `{ field: v }`.
//!   Equality only; non-eq operators error at render time. Used by
//!   hand-rolled schemas like the SpaceX public API.
//!
//! Postgraphile / Relay-cursor styles can be added as further dialects
//! without changing the condition surface.

use serde_json::{Map, Value};
use vantage_core::{Result, error};
use vantage_expressions::{DeferredFn, Expression, Expressive, ExpressiveEnum};

use crate::graphql::types::AnyGraphqlType;

/// How a `GraphqlCondition` is rendered into a GraphQL argument object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterDialect {
    /// Hasura-style: `{ field: { _eq: v } }`, `_and`/`_or`/`_not`.
    Hasura,
    /// Flat-argument schemas like SpaceX: `{ field: v }`. Equality
    /// only — non-eq operators fail at render time.
    Generic,
}

/// The set of comparison/logical operators a `FieldCondition` can use.
///
/// Whether a given dialect can render a given op is decided at render
/// time — see `GraphqlCondition::render`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphqlOp {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    In,
    NotIn,
    Like,
    ILike,
    IsNull,
    IsNotNull,
}

impl GraphqlOp {
    /// Hasura operator name (`_eq`, `_gt`, …). Returns `None` for ops
    /// Hasura can't express verbatim.
    pub fn hasura_key(&self) -> Option<&'static str> {
        Some(match self {
            Self::Eq => "_eq",
            Self::Ne => "_neq",
            Self::Gt => "_gt",
            Self::Gte => "_gte",
            Self::Lt => "_lt",
            Self::Lte => "_lte",
            Self::In => "_in",
            Self::NotIn => "_nin",
            Self::Like => "_like",
            Self::ILike => "_ilike",
            Self::IsNull => "_is_null",
            Self::IsNotNull => "_is_null",
        })
    }
}

/// A single `field <op> value` clause.
#[derive(Clone, Debug)]
pub struct FieldCondition {
    pub field: String,
    pub op: GraphqlOp,
    pub value: Value,
}

impl FieldCondition {
    pub fn new(field: impl Into<String>, op: GraphqlOp, value: Value) -> Self {
        Self {
            field: field.into(),
            op,
            value,
        }
    }
}

/// Structured filter for GraphQL requests. Built by the operator trait
/// (`GraphqlOperation` in `operation.rs`) and rendered at fetch time.
#[derive(Clone)]
pub enum GraphqlCondition {
    Field(FieldCondition),
    /// Like [`Self::Field`] but the value is resolved at fetch time.
    /// Used by relationship traversal — `with_many`/`with_one` builds
    /// one of these when the parent's foreign-key value isn't known
    /// until the parent is fetched. The deferred resolves to a scalar
    /// (the FK value); render-time wraps it in the dialect's `_eq`-
    /// equivalent and merges it into the filter.
    DeferredField {
        field: String,
        op: GraphqlOp,
        value_fn: DeferredFn<AnyGraphqlType>,
    },
    And(Vec<GraphqlCondition>),
    Or(Vec<GraphqlCondition>),
    Not(Box<GraphqlCondition>),
    /// Resolved at fetch time — produces a complete filter sub-object
    /// that already matches the surrounding dialect. Use [`Self::DeferredField`]
    /// instead unless you genuinely need to compute a non-`field op value`
    /// shape dynamically.
    Deferred(DeferredFn<AnyGraphqlType>),
}

impl std::fmt::Debug for GraphqlCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Field(fc) => write!(f, "Field({:?} {:?} {})", fc.field, fc.op, fc.value),
            Self::DeferredField { field, op, .. } => {
                write!(f, "DeferredField({:?} {:?} <pending>)", field, op)
            }
            Self::And(parts) => f.debug_tuple("And").field(parts).finish(),
            Self::Or(parts) => f.debug_tuple("Or").field(parts).finish(),
            Self::Not(inner) => f.debug_tuple("Not").field(inner).finish(),
            Self::Deferred(_) => write!(f, "Deferred(..)"),
        }
    }
}

impl GraphqlCondition {
    /// Build a simple `field = value` condition. Convenience for callers
    /// that have a value already converted to JSON.
    pub fn eq(field: impl Into<String>, value: impl Into<Value>) -> Self {
        Self::Field(FieldCondition::new(field, GraphqlOp::Eq, value.into()))
    }

    /// Render this condition as a JSON object suitable for use as a
    /// GraphQL argument (typically the `where:` arg in Hasura, or the
    /// `find:` arg in flat-argument schemas).
    ///
    /// Deferred branches are resolved here. Resolution may make
    /// out-of-band fetches (e.g. for relationship traversal), so the
    /// method is async.
    pub fn render<'a>(
        &'a self,
        dialect: FilterDialect,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value>> + Send + 'a>> {
        Box::pin(async move {
            match self {
                Self::Field(fc) => render_field(fc, dialect),
                Self::DeferredField { field, op, value_fn } => {
                    let resolved = value_fn.call().await?;
                    let value = match resolved {
                        ExpressiveEnum::Scalar(v) => v.into_value(),
                        other => {
                            return Err(error!(
                                "DeferredField resolved to non-scalar",
                                got = format!("{:?}", other)
                            ));
                        }
                    };
                    let fc = FieldCondition::new(field.clone(), op.clone(), value);
                    render_field(&fc, dialect)
                }
                Self::And(parts) => {
                    let mut rendered = Vec::with_capacity(parts.len());
                    for p in parts {
                        rendered.push(p.render(dialect).await?);
                    }
                    combine_and(rendered, dialect)
                }
                Self::Or(parts) => {
                    if matches!(dialect, FilterDialect::Generic) {
                        return Err(error!(
                            "Generic dialect does not support OR; switch to Hasura"
                        ));
                    }
                    let mut rendered = Vec::with_capacity(parts.len());
                    for p in parts {
                        rendered.push(p.render(dialect).await?);
                    }
                    Ok(Value::Object({
                        let mut m = Map::new();
                        m.insert("_or".into(), Value::Array(rendered));
                        m
                    }))
                }
                Self::Not(inner) => {
                    if matches!(dialect, FilterDialect::Generic) {
                        return Err(error!(
                            "Generic dialect does not support NOT; switch to Hasura"
                        ));
                    }
                    let inner_rendered = inner.render(dialect).await?;
                    Ok(Value::Object({
                        let mut m = Map::new();
                        m.insert("_not".into(), inner_rendered);
                        m
                    }))
                }
                Self::Deferred(deferred) => {
                    let resolved = deferred.call().await?;
                    let inner = match resolved {
                        ExpressiveEnum::Scalar(v) => v.into_value(),
                        other => {
                            return Err(error!(
                                "GraphqlCondition::Deferred resolved to non-scalar",
                                got = format!("{:?}", other)
                            ));
                        }
                    };
                    match inner {
                        Value::Object(_) => Ok(inner),
                        other => Err(error!(
                            "Deferred condition must resolve to a JSON object",
                            got = format!("{:?}", other)
                        )),
                    }
                }
            }
        })
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

fn render_field(fc: &FieldCondition, dialect: FilterDialect) -> Result<Value> {
    match dialect {
        FilterDialect::Hasura => {
            let mut inner = Map::new();
            let key = fc.op.hasura_key().ok_or_else(|| {
                error!("Operator not supported in Hasura dialect", op = format!("{:?}", fc.op))
            })?;
            // Hasura's `_is_null` op takes a Bool; map IsNull → true, IsNotNull → false.
            let value = match fc.op {
                GraphqlOp::IsNull => Value::Bool(true),
                GraphqlOp::IsNotNull => Value::Bool(false),
                _ => fc.value.clone(),
            };
            inner.insert(key.into(), value);
            let mut outer = Map::new();
            outer.insert(fc.field.clone(), Value::Object(inner));
            Ok(Value::Object(outer))
        }
        FilterDialect::Generic => {
            if fc.op != GraphqlOp::Eq {
                return Err(error!(
                    "Generic dialect supports only equality; got non-eq operator",
                    field = fc.field.clone(),
                    op = format!("{:?}", fc.op)
                ));
            }
            let mut m = Map::new();
            m.insert(fc.field.clone(), fc.value.clone());
            Ok(Value::Object(m))
        }
    }
}

/// AND-combine rendered sub-conditions according to dialect.
fn combine_and(parts: Vec<Value>, dialect: FilterDialect) -> Result<Value> {
    match dialect {
        FilterDialect::Hasura => {
            // Hasura allows merging field-keys directly: { foo: {_eq: 1}, bar: {_eq: 2} }
            // is implicit AND. Use _and only when there are duplicate keys.
            let mut merged = Map::new();
            let mut collision = false;
            for p in &parts {
                if let Value::Object(obj) = p {
                    for k in obj.keys() {
                        if merged.contains_key(k) {
                            collision = true;
                            break;
                        }
                    }
                    if collision {
                        break;
                    }
                    if let Value::Object(obj) = p.clone() {
                        for (k, v) in obj {
                            merged.insert(k, v);
                        }
                    }
                }
            }
            if collision {
                Ok(Value::Object({
                    let mut m = Map::new();
                    m.insert("_and".into(), Value::Array(parts));
                    m
                }))
            } else {
                Ok(Value::Object(merged))
            }
        }
        FilterDialect::Generic => {
            // Flat-args schemas only support implicit AND via a shared
            // object. If any key collides, the dialect can't represent
            // it — surface that as an error rather than silently picking
            // a winner.
            let mut merged = Map::new();
            for p in parts {
                if let Value::Object(obj) = p {
                    for (k, v) in obj {
                        if merged.contains_key(&k) {
                            return Err(error!(
                                "Generic dialect can't express two conditions on the same field",
                                field = k
                            ));
                        }
                        merged.insert(k, v);
                    }
                }
            }
            Ok(Value::Object(merged))
        }
    }
}

// ── Conversions ──────────────────────────────────────────────────────

impl From<FieldCondition> for GraphqlCondition {
    fn from(fc: FieldCondition) -> Self {
        Self::Field(fc)
    }
}

/// `GraphqlCondition` is `Expressive` so it satisfies the blanket
/// bound on the operation trait — `cond.eq(false)` shape works the
/// same way as Mongo's chaining.
impl Expressive<AnyGraphqlType> for GraphqlCondition {
    fn expr(&self) -> Expression<AnyGraphqlType> {
        Expression::new(format!("{:?}", self), vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn hasura_renders_eq_as_underscore_eq() {
        let c = GraphqlCondition::Field(FieldCondition::new(
            "mission_name",
            GraphqlOp::Eq,
            json!("FalconSat"),
        ));
        let r = c.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(r, json!({ "mission_name": { "_eq": "FalconSat" } }));
    }

    #[tokio::test]
    async fn generic_renders_eq_as_flat_field() {
        let c = GraphqlCondition::Field(FieldCondition::new(
            "mission_name",
            GraphqlOp::Eq,
            json!("FalconSat"),
        ));
        let r = c.render(FilterDialect::Generic).await.unwrap();
        assert_eq!(r, json!({ "mission_name": "FalconSat" }));
    }

    #[tokio::test]
    async fn generic_rejects_non_eq() {
        let c = GraphqlCondition::Field(FieldCondition::new("price", GraphqlOp::Gt, json!(100)));
        let err = c.render(FilterDialect::Generic).await.unwrap_err();
        assert!(err.to_string().contains("equality"));
    }

    #[tokio::test]
    async fn hasura_renders_gt() {
        let c = GraphqlCondition::Field(FieldCondition::new("price", GraphqlOp::Gt, json!(100)));
        let r = c.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(r, json!({ "price": { "_gt": 100 } }));
    }

    #[tokio::test]
    async fn hasura_renders_is_null_with_bool_arg() {
        let c = GraphqlCondition::Field(FieldCondition::new("deleted_at", GraphqlOp::IsNull, Value::Null));
        let r = c.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(r, json!({ "deleted_at": { "_is_null": true } }));
    }

    #[tokio::test]
    async fn hasura_and_with_distinct_fields_merges_flat() {
        let c = GraphqlCondition::And(vec![
            GraphqlCondition::Field(FieldCondition::new("name", GraphqlOp::Eq, json!("Alice"))),
            GraphqlCondition::Field(FieldCondition::new("active", GraphqlOp::Eq, json!(true))),
        ]);
        let r = c.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(
            r,
            json!({ "name": { "_eq": "Alice" }, "active": { "_eq": true } })
        );
    }

    #[tokio::test]
    async fn hasura_and_with_same_field_uses_explicit_and() {
        let c = GraphqlCondition::And(vec![
            GraphqlCondition::Field(FieldCondition::new("price", GraphqlOp::Gt, json!(10))),
            GraphqlCondition::Field(FieldCondition::new("price", GraphqlOp::Lt, json!(100))),
        ]);
        let r = c.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(
            r,
            json!({
                "_and": [
                    { "price": { "_gt": 10 } },
                    { "price": { "_lt": 100 } }
                ]
            })
        );
    }

    #[tokio::test]
    async fn generic_and_with_same_field_errors() {
        let c = GraphqlCondition::And(vec![
            GraphqlCondition::Field(FieldCondition::new("price", GraphqlOp::Eq, json!(10))),
            GraphqlCondition::Field(FieldCondition::new("price", GraphqlOp::Eq, json!(20))),
        ]);
        let err = c.render(FilterDialect::Generic).await.unwrap_err();
        assert!(err.to_string().contains("same field"));
    }

    #[tokio::test]
    async fn hasura_or_and_not() {
        let c = GraphqlCondition::Not(Box::new(GraphqlCondition::Or(vec![
            GraphqlCondition::Field(FieldCondition::new("active", GraphqlOp::Eq, json!(true))),
            GraphqlCondition::Field(FieldCondition::new("count", GraphqlOp::Gt, json!(0))),
        ])));
        let r = c.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(
            r,
            json!({
                "_not": {
                    "_or": [
                        { "active": { "_eq": true } },
                        { "count": { "_gt": 0 } }
                    ]
                }
            })
        );
    }

    #[tokio::test]
    async fn generic_rejects_or() {
        let c = GraphqlCondition::Or(vec![
            GraphqlCondition::Field(FieldCondition::new("a", GraphqlOp::Eq, json!(1))),
            GraphqlCondition::Field(FieldCondition::new("b", GraphqlOp::Eq, json!(2))),
        ]);
        let err = c.render(FilterDialect::Generic).await.unwrap_err();
        assert!(err.to_string().contains("OR"));
    }
}
