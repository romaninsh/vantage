use async_trait::async_trait;

use vantage_core::Entity;

use crate::{
    dataset::{Id, ReadableDataSet, Result},
    im::Table,
};
use vantage_core::util::error::{Context, vantage_error};

#[async_trait]
impl<E> ReadableDataSet<E> for Table<E>
where
    E: Entity,
{
    async fn get(&self) -> Result<Vec<E>> {
        self.get_as().await
    }
    async fn get_some(&self) -> Result<Option<E>> {
        self.get_some_as().await
    }

    async fn get_id(&self, id: impl Id) -> Result<E> {
        let id = id.into();
        let table = self.data_source.get_or_create_table(&self.table_name);

        match table.get(&id) {
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
    async fn get_as<U>(&self) -> Result<Vec<U>>
    where
        U: Entity,
    {
        let table = self.data_source.get_or_create_table(&self.table_name);
        let mut records = Vec::new();

        for (id, value) in table.iter() {
            // Add the id field to the record for deserialization
            let mut value_with_id = value.clone();
            if let serde_json::Value::Object(ref mut map) = value_with_id {
                map.insert("id".to_string(), serde_json::Value::String(id.clone()));
            }

            let record: U =
                serde_json::from_value(value_with_id).context("Failed to deserialize record")?;
            records.push(record);
        }

        Ok(records)
    }

    async fn get_some_as<U>(&self) -> Result<Option<U>>
    where
        U: Entity,
    {
        let table = self.data_source.get_or_create_table(&self.table_name);

        if let Some((id, value)) = table.iter().next() {
            // Add the id field to the record for deserialization
            let mut value_with_id = value.clone();
            if let serde_json::Value::Object(ref mut map) = value_with_id {
                map.insert("id".to_string(), serde_json::Value::String(id.clone()));
            }

            let record: U =
                serde_json::from_value(value_with_id).context("Failed to deserialize record")?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        let table = self.data_source.get_or_create_table(&self.table_name);
        let mut records = Vec::new();

        for (id, value) in table.iter() {
            // Add the id field to the record
            let mut value_with_id = value.clone();
            if let serde_json::Value::Object(ref mut map) = value_with_id {
                map.insert("id".to_string(), serde_json::Value::String(id.clone()));
            }
            records.push(value_with_id);
        }

        Ok(records)
    }
}
