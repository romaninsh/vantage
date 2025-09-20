use serde_json::Value;
use std::marker::PhantomData;
use vantage_expressions::AssociatedQueryable;
use vantage_expressions::protocol::result::{self, QueryResult};
use vantage_expressions::util::error::{Error, Result};
use vantage_table::Entity;

use crate::select::SurrealSelect;
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
