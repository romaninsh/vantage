use async_trait::async_trait;
use serde::de::DeserializeOwned;

use crate::{
    dataset::{DataSetError, ReadableDataSet, Result},
    im::Table,
};

#[async_trait]
impl<T> ReadableDataSet<T> for Table<T>
where
    T: DeserializeOwned + Send + Sync,
{
    async fn get(&self) -> Result<Vec<T>> {
        self.get_as().await
    }
    async fn get_some(&self) -> Result<Option<T>> {
        self.get_some_as().await
    }
    async fn get_as<U>(&self) -> Result<Vec<U>>
    where
        U: DeserializeOwned,
    {
        let table = self.data_source.get_or_create_table(&self.table_name);
        let mut records = Vec::new();

        for (id, value) in table.iter() {
            // Add the id field to the record for deserialization
            let mut value_with_id = value.clone();
            if let serde_json::Value::Object(ref mut map) = value_with_id {
                map.insert("id".to_string(), serde_json::Value::String(id.clone()));
            }

            let record: U = serde_json::from_value(value_with_id)
                .map_err(|e| DataSetError::other(format!("Deserialization error: {}", e)))?;
            records.push(record);
        }

        Ok(records)
    }

    async fn get_some_as<U>(&self) -> Result<Option<U>>
    where
        U: DeserializeOwned,
    {
        let table = self.data_source.get_or_create_table(&self.table_name);

        if let Some((id, value)) = table.iter().next() {
            // Add the id field to the record for deserialization
            let mut value_with_id = value.clone();
            if let serde_json::Value::Object(ref mut map) = value_with_id {
                map.insert("id".to_string(), serde_json::Value::String(id.clone()));
            }

            let record: U = serde_json::from_value(value_with_id)
                .map_err(|e| DataSetError::other(format!("Deserialization error: {}", e)))?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }
}
