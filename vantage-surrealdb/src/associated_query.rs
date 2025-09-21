use serde_json::Value;
use std::marker::PhantomData;
use std::ops::Deref;
use vantage_expressions::protocol::result::{self, QueryResult};
use vantage_expressions::util::error::{Error, Result};
use vantage_expressions::{AssociatedQueryable, Expression, Selectable};
use vantage_table::Entity;

use crate::select::SurrealSelect;
use crate::surreal_return::SurrealReturn;
use crate::{SurrealDB, protocol::SurrealQueriable};

/// SurrealDB-specific trait for associated queries that can be executed
/// This trait extends AssociatedQueryable with Value conversion
#[async_trait::async_trait]
pub trait SurrealAssociatedQueryable<R>: AssociatedQueryable<R> {
    /// Execute the query and return raw Value
    async fn get_as_value(&self) -> Result<Value>;
}

/// SurrealDB-specific associated query that combines any SurrealQueriable with SurrealDB datasource
pub struct SurrealAssociated<Q: SurrealQueriable, R> {
    pub query: Q,
    pub datasource: SurrealDB,
    _result: PhantomData<R>,
}

impl<Q: SurrealQueriable + std::fmt::Debug, R> std::fmt::Debug for SurrealAssociated<Q, R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurrealAssociated")
            .field("query", &self.query)
            .field("datasource", &"SurrealDB{...}")
            .finish()
    }
}

impl<Q: SurrealQueriable, R> SurrealAssociated<Q, R> {
    pub fn new(query: Q, datasource: SurrealDB) -> Self {
        Self {
            query,
            datasource,
            _result: PhantomData,
        }
    }
}

// Preview method implementation for SurrealSelect types
impl<T: QueryResult, R> SurrealAssociated<SurrealSelect<T>, R> {
    pub fn preview(&self) -> String {
        self.query.preview()
    }
}

impl<Q: SurrealQueriable, R> Deref for SurrealAssociated<Q, R> {
    type Target = Q;

    fn deref(&self) -> &Self::Target {
        &self.query
    }
}

impl<Q: SurrealQueriable + Selectable, R: Send + Sync> Selectable for SurrealAssociated<Q, R> {
    fn set_source(&mut self, source: impl Into<vantage_expressions::Expr>, alias: Option<String>) {
        self.query.set_source(source, alias)
    }

    fn add_field(&mut self, field: impl Into<String>) {
        self.query.add_field(field)
    }

    fn add_expression(
        &mut self,
        expression: vantage_expressions::Expression,
        alias: Option<String>,
    ) {
        self.query.add_expression(expression, alias)
    }

    fn add_where_condition(&mut self, condition: vantage_expressions::Expression) {
        self.query.add_where_condition(condition)
    }

    fn set_distinct(&mut self, distinct: bool) {
        self.query.set_distinct(distinct)
    }

    fn add_order_by(
        &mut self,
        field_or_expr: impl Into<vantage_expressions::Expr>,
        ascending: bool,
    ) {
        self.query.add_order_by(field_or_expr, ascending)
    }

    fn add_group_by(&mut self, expression: vantage_expressions::Expression) {
        self.query.add_group_by(expression)
    }

    fn set_limit(&mut self, limit: Option<i64>, skip: Option<i64>) {
        self.query.set_limit(limit, skip)
    }

    fn clear_fields(&mut self) {
        self.query.clear_fields()
    }

    fn clear_where_conditions(&mut self) {
        self.query.clear_where_conditions()
    }

    fn clear_order_by(&mut self) {
        self.query.clear_order_by()
    }

    fn clear_group_by(&mut self) {
        self.query.clear_group_by()
    }

    fn has_fields(&self) -> bool {
        self.query.has_fields()
    }

    fn has_where_conditions(&self) -> bool {
        self.query.has_where_conditions()
    }

    fn has_order_by(&self) -> bool {
        self.query.has_order_by()
    }

    fn has_group_by(&self) -> bool {
        self.query.has_group_by()
    }

    fn is_distinct(&self) -> bool {
        self.query.is_distinct()
    }

    fn get_limit(&self) -> Option<i64> {
        self.query.get_limit()
    }

    fn get_skip(&self) -> Option<i64> {
        self.query.get_skip()
    }
}

impl<Q: SurrealQueriable + Into<Expression>, R> Into<Expression> for SurrealAssociated<Q, R> {
    fn into(self) -> Expression {
        self.query.into()
    }
}

// Implementation for SurrealSelect<result::Rows> - returns Vec<Entity>
#[async_trait::async_trait]
impl<E: Entity> AssociatedQueryable<Vec<E>>
    for SurrealAssociated<SurrealSelect<result::Rows>, Vec<E>>
{
    async fn get(&self) -> Result<Vec<E>> {
        let raw_result = self.query.get(&self.datasource).await;
        let entities = raw_result
            .into_iter()
            .map(|item| serde_json::from_value(Value::Object(item)))
            .collect::<std::result::Result<Vec<E>, _>>()
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(entities)
    }
}

#[async_trait::async_trait]
impl<E: Entity> SurrealAssociatedQueryable<Vec<E>>
    for SurrealAssociated<SurrealSelect<result::Rows>, Vec<E>>
{
    async fn get_as_value(&self) -> Result<Value> {
        let raw_result = self.query.get(&self.datasource).await;
        let json_value = Value::Array(
            raw_result
                .into_iter()
                .map(|map| Value::Object(map))
                .collect(),
        );
        Ok(json_value)
    }
}

// Implementation for SurrealSelect<result::SingleRow> - returns Entity
#[async_trait::async_trait]
impl<E: Entity> AssociatedQueryable<E> for SurrealAssociated<SurrealSelect<result::SingleRow>, E> {
    async fn get(&self) -> Result<E> {
        let raw_result = self.query.get(&self.datasource).await;
        let entity: E = serde_json::from_value(Value::Object(raw_result))
            .map_err(|e| Error::new(e.to_string()))?;
        Ok(entity)
    }
}

#[async_trait::async_trait]
impl<E: Entity> SurrealAssociatedQueryable<E>
    for SurrealAssociated<SurrealSelect<result::SingleRow>, E>
{
    async fn get_as_value(&self) -> Result<Value> {
        let raw_result = self.query.get(&self.datasource).await;
        Ok(Value::Object(raw_result))
    }
}

// Implementation for SurrealSelect<result::List> - returns Vec<Value>
#[async_trait::async_trait]
impl AssociatedQueryable<Vec<Value>>
    for SurrealAssociated<SurrealSelect<result::List>, Vec<Value>>
{
    async fn get(&self) -> Result<Vec<Value>> {
        let raw_result = self.query.get(&self.datasource).await;
        Ok(raw_result)
    }
}

#[async_trait::async_trait]
impl SurrealAssociatedQueryable<Vec<Value>>
    for SurrealAssociated<SurrealSelect<result::List>, Vec<Value>>
{
    async fn get_as_value(&self) -> Result<Value> {
        let raw_result = self.query.get(&self.datasource).await;
        Ok(Value::Array(raw_result))
    }
}

// Implementation for SurrealSelect<result::Single> - returns Value
#[async_trait::async_trait]
impl AssociatedQueryable<Value> for SurrealAssociated<SurrealSelect<result::Single>, Value> {
    async fn get(&self) -> Result<Value> {
        let raw_result = self.query.get(&self.datasource).await;
        Ok(raw_result)
    }
}

#[async_trait::async_trait]
impl SurrealAssociatedQueryable<Value> for SurrealAssociated<SurrealSelect<result::Single>, Value> {
    async fn get_as_value(&self) -> Result<Value> {
        let raw_result = self.query.get(&self.datasource).await;
        Ok(raw_result)
    }
}

// Implementation for SurrealReturn - returns Value
#[async_trait::async_trait]
impl AssociatedQueryable<Value> for SurrealAssociated<SurrealReturn, Value> {
    async fn get(&self) -> Result<Value> {
        let raw_result = self.query.get(&self.datasource).await;
        Ok(raw_result)
    }
}

#[async_trait::async_trait]
impl SurrealAssociatedQueryable<Value> for SurrealAssociated<SurrealReturn, Value> {
    async fn get_as_value(&self) -> Result<Value> {
        let raw_result = self.query.get(&self.datasource).await;
        Ok(raw_result)
    }
}

// Implementation for SurrealReturn - returns i64 (for count queries)
#[async_trait::async_trait]
impl AssociatedQueryable<i64> for SurrealAssociated<SurrealReturn, i64> {
    async fn get(&self) -> Result<i64> {
        let raw_result = self.query.get(&self.datasource).await;
        // SurrealDB count returns [number], so extract the first element
        if let Value::Array(ref arr) = raw_result {
            if let Some(Value::Number(num)) = arr.first() {
                if let Some(count) = num.as_i64() {
                    return Ok(count);
                }
            }
        }
        Err(Error::new(
            "Failed to parse count result as i64".to_string(),
        ))
    }
}

#[async_trait::async_trait]
impl SurrealAssociatedQueryable<i64> for SurrealAssociated<SurrealReturn, i64> {
    async fn get_as_value(&self) -> Result<Value> {
        let raw_result = self.query.get(&self.datasource).await;
        Ok(raw_result)
    }
}
