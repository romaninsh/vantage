#![allow(dead_code)]

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crate::dataset::ReadableDataSet;
use crate::prelude::{EmptyEntity, Entity};
use crate::sql::Query;
use crate::sql::chunk::Chunk;
use crate::sql::expression::{Expression, ExpressionArc};
use crate::sql::query::SqlQuery;
use crate::traits::datasource::DataSource;
use anyhow::Context;
use anyhow::{Result, anyhow};
use futures::{TryStreamExt, pin_mut};
use indexmap::IndexMap;
use rust_decimal::Decimal;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;
use tokio_postgres::Client;
use tokio_postgres::Row;
use tokio_postgres::types::ToSql;

#[derive(Clone, Debug)]
pub struct Postgres {
    client: Arc<Box<Client>>,
}

/// Postgres is equal to its clones.
impl PartialEq for Postgres {
    fn eq(&self, other: &Postgres) -> bool {
        Arc::ptr_eq(&self.client, &other.client)
    }
}

impl Postgres {
    pub fn new(client: Arc<Box<Client>>) -> Postgres {
        Postgres { client }
    }

    pub fn escape(&self, expr: String) -> String {
        format!("\"{}\"", expr)
    }

    pub fn annotate_type(&self, expr: String, as_type: String) -> String {
        format!("{}::{}", expr, as_type)
    }

    pub fn convert_value_tosql(&self, value: Value) -> Box<dyn ToSql + Sync> {
        match value {
            Value::Null => Box::new(None as Option<bool>),
            Value::Bool(b) => Box::new(b),
            Value::Number(n) => {
                if n.is_i64() {
                    Box::new(n.as_i64().unwrap() as i32)
                } else {
                    Box::new(n.as_f64().unwrap() as f32)
                }
            }
            Value::String(s) => Box::new(s),
            Value::Array(a) => Box::new(serde_json::to_string(&a).unwrap()),
            Value::Object(o) => Box::new(serde_json::to_string(&o).unwrap()),
        }
    }

    pub fn convert_value_fromsql(&self, row: Row) -> Result<Value> {
        let mut json_map: IndexMap<String, Value> = IndexMap::new();

        for (i, col) in row.columns().iter().enumerate() {
            let name = col.name().to_string();
            let col_type = col.type_().name();
            let value = match col_type {
                "int4" => json!(row.get::<_, Option<i32>>(i)), // int4 as i32
                "int8" => json!(row.get::<_, Option<i64>>(i)), // int8 as i64
                "varchar" | "text" => json!(row.get::<_, Option<String>>(i)), // varchar and text as String
                "bool" => json!(row.get::<_, Option<bool>>(i)),               // bool as bool
                "float4" => json!(row.get::<_, Option<f32>>(i)),              // float4 as f32
                "float8" => json!(row.get::<_, Option<f64>>(i)),              // float8 as f64
                "numeric" => json!(row.get::<_, Option<Decimal>>(i)),         // numeric as f64
                // "date" => row
                //     .get::<_, Option<chrono::NaiveDate>>(i)
                //     .map(|d| json!(d.to_string())), // date as ISO8601 string
                // "timestamp" => row
                //     .get::<_, Option<chrono::NaiveDateTime>>(i)
                //     .map(|dt| json!(dt.to_string())), // timestamp as ISO8601 string
                _ => {
                    return Err(anyhow!(
                        "Unsupported type: {} for column {}",
                        col_type,
                        name
                    ));
                }
            };

            json_map.insert(name, value);
        }

        Ok(json!(json_map))
    }

    pub fn client(&self) -> &tokio_postgres::Client {
        self.client.as_ref()
    }

    pub async fn query_into_statement(&self, query: &Query) -> Result<tokio_postgres::Statement> {
        let query_rendered = query.render_chunk();
        self.client
            .prepare(&query_rendered.sql_final())
            .await
            .with_context(|| format!("Attempting to execute query {}", query_rendered.preview()))
    }

    pub async fn query_raw(&self, query: &Query) -> Result<Vec<Value>> {
        let query_rendered = query.render_chunk();
        let params_tosql = query_rendered
            .params()
            .iter()
            .map(|v| self.convert_value_tosql(v.clone()));

        // let params_tosql_refs = params_tosql
        //     .iter()
        //     .map(|b| b.as_ref())
        //     .collect::<Vec<&(dyn ToSql + Sync)>>();

        let result = self
            .client
            .query_raw(&query_rendered.sql_final(), params_tosql)
            .await
            .context(anyhow!("Error in query {}", query.preview()))?;

        pin_mut!(result);
        let mut results = Vec::new();
        while let Some(row) = result.try_next().await? {
            // for row in result {
            results.push(self.convert_value_fromsql(row)?);
        }

        Ok(results)
    }

    pub async fn query_opt(&self, query: &Query) -> Result<Option<Value>> {
        Ok(self.query_raw(query).await?.into_iter().next())
    }
}

trait InsertRows {
    async fn insert_rows(&self, query: &Query, rows: &Vec<Vec<Value>>) -> Result<Vec<Value>>;
}

impl InsertRows for Postgres {
    async fn insert_rows(&self, query: &Query, rows: &Vec<Vec<Value>>) -> Result<Vec<Value>> {
        // no rows to insert
        if rows.len() == 0 {
            return Ok(vec![]);
        }

        let query_rendered = query.render_chunk();
        let num_rows = query_rendered.params().len();

        if rows.len() == 0 {
            return Err(anyhow!("Insert query contains zero fields"));
        }

        let statement = self
            .client
            .prepare(&query_rendered.sql_final())
            .await
            .context("Attempting to execute an insert query")?;

        let mut row_cnt = 0;
        let mut ids = Vec::new();
        for row_set in rows {
            row_cnt += 1;
            if row_set.len() != num_rows {
                return Err(anyhow!(
                    "Number of columns in a row {} does not match number of fields in a query {} at row {}",
                    row_set.len(),
                    num_rows,
                    row_cnt
                ));
            }

            let params_tosql = row_set
                .iter()
                .map(|v| self.convert_value_tosql(v.clone()))
                .collect::<Vec<_>>();

            let params_tosql_refs = params_tosql
                .iter()
                .map(|b| b.as_ref())
                .collect::<Vec<&(dyn ToSql + Sync)>>();

            let row = self
                .client
                .query_one(&statement, params_tosql_refs.as_slice())
                .await?;

            let row = self.convert_value_fromsql(row)?;

            let row = if let Value::Object(obj) = row {
                obj
            } else {
                return Err(anyhow!("Expected query_one to return an Value::Object"));
            };

            let id = row
                .into_iter()
                .next()
                .context("query_one returned empty object")?
                .1;

            ids.push(id)
        }

        Ok(ids)
    }
}

trait SelectRows {
    async fn select_rows(&self, query: &Query) -> Result<Vec<Value>>;
}

impl SelectRows for Postgres {
    async fn select_rows(&self, query: &Query) -> Result<Vec<Value>> {
        // let (sql, params) = query.render_chunks();
        self.query_raw(query).await
    }
}

impl DataSource for Postgres {
    async fn query_fetch(&self, query: &Query) -> Result<Vec<Map<String, Value>>> {
        let res = self.query_raw(query).await?;
        let res = res
            .into_iter()
            .map(|v| v.as_object().unwrap().clone())
            // TODO: unwanted clone
            .collect();
        Ok(res)
    }

    async fn query_exec(&self, query: &Query) -> Result<Option<Value>> {
        let res = self.query_raw(query).await?;
        if res.len() == 0 {
            Ok(None)
        } else {
            Ok(Some(res[0].clone()))
        }
    }

    async fn query_insert(&self, _query: &Query, _rows: Vec<Vec<Value>>) -> Result<()> {
        todo!()
    }
    async fn query_row(&self, query: &Query) -> Result<Map<String, Value>> {
        let Some(Value::Object(res)) = self.query_raw(query).await?.into_iter().next() else {
            return Err(anyhow!("No rows for query_row"));
        };
        Ok(res)
    }
    async fn query_one(&self, query: &Query) -> Result<Value> {
        let Some(Value::Object(res)) = self.query_raw(query).await?.into_iter().next() else {
            return Err(anyhow!("No rows for query_one"));
        };
        let Some((_, res)) = res.into_iter().next() else {
            return Err(anyhow!("No cells in a first row of query_one"));
        };
        Ok(res)
    }
    async fn query_col(&self, query: &Query) -> Result<Vec<Value>> {
        let res = self.query_raw(query).await?;
        let res = res
            .into_iter()
            .filter_map(|v| Some(v.as_object()?.iter().next()?.1.clone()))
            .collect();
        Ok(res)
    }
}

pub struct AssociatedExpressionArc<T: DataSource> {
    pub expr: ExpressionArc,
    pub ds: T,
}

impl<T: DataSource> Deref for AssociatedExpressionArc<T> {
    type Target = ExpressionArc;

    fn deref(&self) -> &Self::Target {
        &self.expr
    }
}

impl<T: DataSource> AssociatedExpressionArc<T> {
    pub fn new(expr: ExpressionArc, ds: T) -> Self {
        Self { expr, ds }
    }
    pub async fn get_one(&self) -> Result<Value> {
        let one = self
            .ds
            .query_one(
                &Query::new().with_type(crate::sql::query::QueryType::Expression(
                    self.expr.render_chunk(),
                )),
            )
            .await?;
        Ok(one)
    }
}

#[cfg(test)]
mod tests {

    // #[tokio::test]
    // async fn test_insert_async() {
    //     let (client, connection) = tokio_postgres::connect("host=localhost dbname=postgres", NoTls)
    //         .await
    //         .unwrap();

    //     tokio::spawn(async move {
    //         if let Err(e) = connection.await {
    //             eprintln!("connection error: {}", e);
    //         }
    //     });

    //     let postgres = Postgres::new(Arc::new(Box::new(client)));

    //     let query = Query::new()
    //         .set_table("client", None)
    //         .set_type(QueryType::Insert)
    //         .add_column_field("name")
    //         .add_column_field("email")
    //         .add_column_field("is_vip");

    //     let rows: Vec<Vec<Value>> = vec![
    //         vec![json!("John"), json!("john@gamil.com"), json!(true)],
    //         vec![json!("Jane"), json!("jave@ffs.org"), json!(true)],
    //     ];

    //     dbg!(&query.render_chunk());
    //     let ids = postgres.insert_rows(&query, &rows).await.unwrap();

    //     // should be sequential
    //     assert!(ids[0].as_i64().unwrap() + 1 == ids[1].as_i64().unwrap());
    //     let id0 = ids[0].as_i64().unwrap() as i32;
    //     let id1 = ids[1].as_i64().unwrap() as i32;

    //     let expr = expr!("id in ({}, {})", id0, id1);

    //     let delete_query = Query::new()
    //         .set_table("client", None)
    //         .set_type(QueryType::Delete)
    //         .add_condition(expr);

    //     postgres.query_raw(&delete_query).await.unwrap();
    // }
}
