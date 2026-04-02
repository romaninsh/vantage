use crate::{
    dataset::WritableDataSet, prelude::Entity, sql::query::QueryType,
    traits::datasource::DataSource,
};

use super::{AnyTable, Table, TableWithQueries};
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;

// You should be able to insert and delete data in a table
impl<T: DataSource, E: Entity> WritableDataSet<E> for Table<T, E> {
    async fn insert(&self, record: E) -> Result<Option<Value>> {
        let query = self.get_insert_query(record);
        let Some(id) = self.data_source.query_exec(&query).await? else {
            return Ok(None);
        };
        if self.id_column.is_none() {
            return Ok(None);
        }
        let Some(id) = id.get(self.id_column.as_ref().unwrap()) else {
            return Ok(None);
        };
        Ok(Some(id.clone()))
    }

    async fn update<F>(&self, _f: F) -> Result<()> {
        todo!()
    }

    async fn update_with<F, T2>(&self, values: T2) -> Result<()>
    where
        T2: Serialize + Clone,
    {
        // ensure that T2 does not specify ID field
        let Value::Object(values_map) = serde_json::to_value(values.clone()).unwrap() else {
            return Err(anyhow::anyhow!("T2 must be a struct"));
        };

        if let Some(ref id_field) = self.id_column {
            if values_map.get(id_field).is_some() {
                return Err(anyhow::anyhow!("T2 must not specify ID field"));
            }
        }

        let query = self.get_update_query(values);
        self.data_source.query_exec(&query).await.map(|_| ())
    }

    async fn delete(&self) -> Result<()> {
        let mut query = self.get_empty_query().with_type(QueryType::Delete);
        self.hooks().before_delete_query(self, &mut query).unwrap();
        self.data_source.query_exec(&query).await.map(|_| ())
    }
}
