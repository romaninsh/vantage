use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::Entity;

use crate::{
    dataset::{ReadableValueSet, Result},
    im::ImTable,
};

#[async_trait]
impl<E> ReadableValueSet for ImTable<E>
where
    E: Entity,
{
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Self::Value>> {
        let table = self.data_source.get_or_create_table(&self.table_name);
        let mut records = IndexMap::new();

        for (id, value) in table.iter() {
            // Add the id field to the record
            let mut value_with_id = value.clone();
            if let serde_json::Value::Object(ref mut map) = value_with_id {
                map.insert("id".to_string(), serde_json::Value::String(id.clone()));
            }
            records.insert(id.clone(), value_with_id);
        }

        Ok(records)
    }

    async fn get_value(&self, id: &Self::Id) -> Result<Self::Value> {
        let table = self.data_source.get_or_create_table(&self.table_name);

        match table.get(id) {
            Some(value) => {
                // Add the id field to the record
                let mut value_with_id = value.clone();
                if let serde_json::Value::Object(ref mut map) = value_with_id {
                    map.insert("id".to_string(), serde_json::Value::String(id.clone()));
                }
                Ok(value_with_id)
            }
            None => Err(vantage_core::util::error::vantage_error!(
                "Record with id '{}' not found",
                id
            )),
        }
    }

    async fn get_some_value(&self) -> Result<Option<(Self::Id, Self::Value)>> {
        let table = self.data_source.get_or_create_table(&self.table_name);

        if let Some((id, value)) = table.iter().next() {
            // Add the id field to the record
            let mut value_with_id = value.clone();
            if let serde_json::Value::Object(ref mut map) = value_with_id {
                map.insert("id".to_string(), serde_json::Value::String(id.clone()));
            }
            Ok(Some((id.clone(), value_with_id)))
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
    async fn test_list_values() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let result = table.list_values().await.unwrap();
        assert_eq!(result.len(), 0);
    }

    #[tokio::test]
    async fn test_get_value() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let result = table.get_value(&"nonexistent".to_string()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_some_value() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let result = table.get_some_value().await.unwrap();
        assert!(result.is_none());
    }
}
