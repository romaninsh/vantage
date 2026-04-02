use anyhow::Result;
use serde_json::{Map, Value};
use std::ops::{Deref, DerefMut};

use crate::{
    dataset::ReadableDataSet,
    prelude::Entity,
    sql::{Chunk, Expression, Query, query::SqlQuery},
    traits::DataSource,
};

/// While [`Query`] does not generally associate with the [`DataSource`], it may be inconvenient
/// to execute it. AssociatedQuery combines query with the datasource, allowing you to ealily
/// pass it around and execute it.
///
/// ```
/// let clients = Client::table();
/// let client_count = clients.count();   // returns AssociatedQuery
///
/// let cnt: Value = client_count.get_one_untuped().await?;  // actually executes the query
/// ```
///
/// AssociatedQuery can be used to make a link between DataSources:
///
/// ```
/// let clients = Client::table();
/// let client_code_query = clients.field_query(clients.code())?;
/// // returns field query (SELECT code FROM client)
///
/// let orders = Order::table();
/// let orders = orders.with_condition(
///     orders.client_code().in(orders.glue(client_code_query).await?)
/// );
/// ```
/// If Order and Client tables do share same [`DataSource`], the conditioun would be set as
///  `WHERE (client_code IN (SELECT code FROM client))`, ultimatelly saving you from
/// redundant query.
///
/// When datasources are different, [`glue()`] would execute `SELECT code FROM client`, fetch
/// the results and use those as a vector of values in a condition clause:
///  `WHERE (client_code IN [12, 13, 14])`
///
/// [`DataSource`]: crate::traits::datasource::DataSource
/// [`glue()`]: Table::glue
///
#[derive(Clone)]
pub struct AssociatedQuery<T: DataSource, E: Entity> {
    pub query: Query,
    pub ds: T,
    pub _phantom: std::marker::PhantomData<E>,
}
impl<T: DataSource, E: Entity> Deref for AssociatedQuery<T, E> {
    type Target = Query;

    fn deref(&self) -> &Self::Target {
        &self.query
    }
}
impl<T: DataSource, E: Entity> DerefMut for AssociatedQuery<T, E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.query
    }
}

impl<T: DataSource, E: Entity> AssociatedQuery<T, E> {
    pub fn new(query: Query, ds: T) -> Self {
        Self {
            query,
            ds,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn with_skip(mut self, skip: i64) -> Self {
        self.query.add_skip(Some(skip));
        self
    }

    pub fn with_limit(mut self, limit: i64) -> Self {
        self.query.add_limit(Some(limit));
        self
    }

    pub fn with_skip_and_limit(mut self, skip: i64, limit: i64) -> Self {
        self.query.add_limit(Some(limit));
        self.query.add_skip(Some(skip));
        self
    }

    /// Presented with another AssociatedQuery - calculate if queries
    /// are linked with the same or different [`DataSource`]s.
    ///
    /// The same - return expression as-is.
    /// Different - execute the query and return the result as a vector of values.
    async fn glue(&self, other: AssociatedQuery<T, E>) -> Result<Expression> {
        if self.ds.eq(&other.ds) {
            Ok(other.query.render_chunk())
        } else {
            let vals = other.get_col_untyped().await?;
            let tpl = vec!["{}"; vals.len()].join(", ");
            Ok(Expression::new(tpl, vals))
        }
    }
}
impl<D: DataSource + Sync, E: Entity> Chunk for AssociatedQuery<D, E> {
    fn render_chunk(&self) -> Expression {
        self.query.render_chunk()
    }
}
impl<D: DataSource, E: Entity> std::fmt::Debug for AssociatedQuery<D, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssociatedQuery")
            .field("query", &self.query)
            .field("ds", &self.ds)
            .finish()
    }
}
impl<T: DataSource + Sync, E: Entity> ReadableDataSet<E> for AssociatedQuery<T, E> {
    async fn get_all_untyped(&self) -> Result<Vec<Map<String, Value>>> {
        self.ds.query_fetch(&self.query).await
    }

    async fn get_row_untyped(&self) -> Result<Map<String, Value>> {
        self.ds.query_row(&self.query).await
    }

    async fn get_one_untyped(&self) -> Result<Value> {
        self.ds.query_one(&self.query).await
    }

    async fn get_col_untyped(&self) -> Result<Vec<Value>> {
        self.ds.query_col(&self.query).await
    }

    async fn get(&self) -> Result<Vec<E>> {
        let data = self.get_all_untyped().await?;
        Ok(data
            .into_iter()
            .map(|row| serde_json::from_value(Value::Object(row)).unwrap())
            .collect())
    }

    async fn get_as<T2: serde::de::DeserializeOwned>(&self) -> Result<Vec<T2>> {
        let data = self.get_all_untyped().await?;
        Ok(data
            .into_iter()
            .map(|row| serde_json::from_value(Value::Object(row)).unwrap())
            .collect())
    }

    async fn get_some(&self) -> Result<Option<E>> {
        let data = self.ds.query_fetch(&self.query).await?;
        if data.len() > 0 {
            let row = data[0].clone();
            let row = serde_json::from_value(Value::Object(row)).unwrap();
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    async fn get_some_as<T2: serde::de::DeserializeOwned>(&self) -> Result<Option<T2>> {
        let data = self.ds.query_fetch(&self.query).await?;
        if data.len() > 0 {
            let row = data[0].clone();
            let row = serde_json::from_value(Value::Object(row)).unwrap();
            Ok(Some(row))
        } else {
            Ok(None)
        }
    }

    fn select_query(&self) -> Query {
        self.query.clone()
    }
}
