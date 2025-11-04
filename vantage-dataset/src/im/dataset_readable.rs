use async_trait::async_trait;
use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use vantage_core::Entity;

use crate::{
    dataset::{DataSet, ReadableDataSet, Result},
    im::ImTable,
};
use vantage_core::util::error::{Context, vantage_error};

#[async_trait]
impl<E> DataSet<E> for ImTable<E> where E: Entity {}

#[async_trait]
impl<E> ReadableDataSet<E> for ImTable<E>
where
    E: Entity + DeserializeOwned,
{
    async fn list(&self) -> Result<IndexMap<Self::Id, E>> {
        let table = self.data_source.get_or_create_table(&self.table_name);
        let mut records = IndexMap::new();

        for (id, value) in table.iter() {
            // Add the id field to the record for deserialization
            let mut value_with_id = value.clone();
            if let serde_json::Value::Object(ref mut map) = value_with_id {
                map.insert("id".to_string(), serde_json::Value::String(id.clone()));
            }

            let record: E =
                serde_json::from_value(value_with_id).context("Failed to deserialize record")?;
            records.insert(id.clone(), record);
        }

        Ok(records)
    }

    async fn get(&self, id: &Self::Id) -> Result<E> {
        let table = self.data_source.get_or_create_table(&self.table_name);

        match table.get(id) {
            Some(value) => {
                // Add the id field to the record for deserialization
                let mut value_with_id = value.clone();
                if let serde_json::Value::Object(ref mut map) = value_with_id {
                    map.insert("id".to_string(), serde_json::Value::String(id.clone()));
                }

                serde_json::from_value(value_with_id).context("Failed to deserialize record")
            }
            None => Err(vantage_error!("Record with id '{}' not found", id)),
        }
    }

    async fn get_some(&self) -> Result<Option<(Self::Id, E)>> {
        let table = self.data_source.get_or_create_table(&self.table_name);

        if let Some((id, value)) = table.iter().next() {
            // Add the id field to the record for deserialization
            let mut value_with_id = value.clone();
            if let serde_json::Value::Object(ref mut map) = value_with_id {
                map.insert("id".to_string(), serde_json::Value::String(id.clone()));
            }

            let record: E =
                serde_json::from_value(value_with_id).context("Failed to deserialize record")?;
            Ok(Some((id.clone(), record)))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::im::ImDataSource;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    struct User {
        id: Option<String>,
        name: String,
    }

    #[tokio::test]
    async fn test_list() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let result = table.list().await.unwrap();
        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_get() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let result = table.get(&"nonexistent".to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_some() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let result = table.get_some().await.unwrap();
        assert!(result.is_none());
    }
}
