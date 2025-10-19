use super::MockColumn;
use crate::{TableLike, TableSource};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use vantage_core::util::error::{Context, vantage_error};
use vantage_dataset::{dataset::Result, prelude::VantageError};
use vantage_expressions::protocol::datasource::DataSource;

pub struct MockTableSource {
    data: Arc<Mutex<HashMap<String, Vec<serde_json::Value>>>>,
}

impl MockTableSource {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_data(self, table_name: &str, data: Vec<serde_json::Value>) -> Self {
        self.data
            .lock()
            .unwrap()
            .insert(table_name.to_string(), data);
        self
    }
}

impl Default for MockTableSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for MockTableSource {}

#[async_trait]
impl TableSource for MockTableSource {
    type Column = MockColumn;
    type Expr = vantage_expressions::Expression;

    fn create_column(&self, name: &str, _table: impl TableLike) -> Self::Column {
        MockColumn::new(name)
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<vantage_expressions::protocol::expressive::IntoExpressive<Self::Expr>>,
    ) -> Self::Expr {
        vantage_expressions::Expression::new(template, parameters)
    }

    async fn get_table_data<E>(&self, table: &crate::Table<Self, E>) -> Result<Vec<(String, E)>>
    where
        E: crate::Entity,
        Self: Sized,
    {
        let values = self.get_table_data_as_value(table).await?;
        let mut results = Vec::new();

        for value in values {
            let id = value
                .get("id")
                .and_then(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .or_else(|| v.as_i64().map(|i| i.to_string()))
                        .or_else(|| v.as_u64().map(|u| u.to_string()))
                })
                .unwrap_or_else(|| "unknown".to_string());

            match serde_json::from_value::<E>(value) {
                Ok(item) => results.push((id, item)),
                Err(e) => {
                    return Err(vantage_error!("Failed to deserialize entity: {}", e));
                }
            }
        }

        Ok(results)
    }

    async fn get_table_data_some<E>(
        &self,
        table: &crate::Table<Self, E>,
    ) -> Result<Option<(String, E)>>
    where
        E: crate::Entity,
        Self: Sized,
    {
        let values = self.get_table_data_as_value(table).await?;

        if let Some(first_value) = values.into_iter().next() {
            let id = first_value
                .get("id")
                .and_then(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .or_else(|| v.as_i64().map(|i| i.to_string()))
                        .or_else(|| v.as_u64().map(|u| u.to_string()))
                })
                .unwrap_or_else(|| "unknown".to_string());

            match serde_json::from_value::<E>(first_value) {
                Ok(item) => Ok(Some((id, item))),
                Err(e) => Err(vantage_error!("Failed to deserialize entity: {}", e)),
            }
        } else {
            Ok(None)
        }
    }

    async fn get_table_data_as_value<E>(
        &self,
        _table: &crate::Table<Self, E>,
    ) -> Result<Vec<serde_json::Value>>
    where
        E: crate::Entity,
        Self: Sized,
    {
        match self.data.lock().unwrap().get(&_table.table_name) {
            Some(data) => Ok(data.clone()),
            None => Ok(vec![]),
        }
    }

    async fn get_table_data_as_value_by_id<E>(
        &self,
        table: &crate::Table<Self, E>,
        id: &str,
    ) -> Result<serde_json::Value>
    where
        E: crate::Entity,
        Self: Sized,
    {
        let data = self.data.lock().unwrap();
        let vec = data.get(&table.table_name).ok_or(VantageError::no_data())?;

        for value in vec {
            if let Some(record_id) = value.get("id") {
                let record_id_str = record_id
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| record_id.as_i64().map(|i| i.to_string()))
                    .or_else(|| record_id.as_u64().map(|u| u.to_string()))
                    .unwrap_or_else(|| "unknown".to_string());

                if record_id_str == id {
                    return Ok(value.clone());
                }
            }
        }

        Err(vantage_error!("No record found with ID: {}", id))
    }

    async fn get_table_data_as_value_some<E>(
        &self,
        table: &crate::Table<Self, E>,
    ) -> Result<Option<serde_json::Value>>
    where
        E: crate::Entity,
        Self: Sized,
    {
        match self.data.lock().unwrap().get(&table.table_name) {
            Some(data) => Ok(data.first().cloned()),
            None => Ok(None),
        }
    }

    async fn insert_table_data<E>(
        &self,
        table: &crate::Table<Self, E>,
        record: E,
    ) -> Result<Option<String>>
    where
        E: crate::Entity + serde::Serialize,
        Self: Sized,
    {
        let mut data = self.data.lock().unwrap();
        let vec = data
            .get_mut(&table.table_name)
            .ok_or(VantageError::no_data())?;
        let id = vec.len();
        let value = serde_json::to_value(record).context("Failed to serialize record")?;
        vec.push(value);
        Ok(Some(id.to_string()))
    }

    async fn insert_table_data_with_id<E>(
        &self,
        _table: &crate::Table<Self, E>,
        _id: impl vantage_dataset::dataset::Id,
        _record: E,
    ) -> Result<()>
    where
        E: crate::Entity + serde::Serialize,
        Self: Sized,
    {
        Err(vantage_error!(
            "insert_table_data_with_id not implemented in mock"
        ))
    }

    async fn replace_table_data_with_id<E>(
        &self,
        table: &crate::Table<Self, E>,
        id: impl vantage_dataset::dataset::Id,
        record: E,
    ) -> Result<()>
    where
        E: crate::Entity + serde::Serialize,
        Self: Sized,
    {
        let id_str = id.into();
        let mut data = self.data.lock().unwrap();
        let vec = data
            .get_mut(&table.table_name)
            .ok_or(VantageError::no_data())?;

        // Find the record with matching ID
        let mut found_index = None;
        for (index, value) in vec.iter().enumerate() {
            if let Some(record_id) = value.get("id") {
                let record_id_str = record_id
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| record_id.as_i64().map(|i| i.to_string()))
                    .or_else(|| record_id.as_u64().map(|u| u.to_string()))
                    .unwrap_or_else(|| "unknown".to_string());

                if record_id_str == id_str {
                    found_index = Some(index);
                    break;
                }
            }
        }

        let index =
            found_index.ok_or_else(|| vantage_error!("No record found with ID: {}", id_str))?;

        let value = serde_json::to_value(record).context("Failed to serialize record")?;
        vec[index] = value;
        Ok(())
    }

    async fn patch_table_data_with_id<E>(
        &self,
        _table: &crate::Table<Self, E>,
        _id: impl vantage_dataset::dataset::Id,
        _partial: serde_json::Value,
    ) -> Result<()>
    where
        E: crate::Entity,
        Self: Sized,
    {
        Err(vantage_error!(
            "patch_table_data_with_id not implemented in mock"
        ))
    }

    async fn delete_table_data_with_id<E>(
        &self,
        _table: &crate::Table<Self, E>,
        _id: impl vantage_dataset::dataset::Id,
    ) -> Result<()>
    where
        E: crate::Entity,
        Self: Sized,
    {
        Err(vantage_error!(
            "delete_table_data_with_id not implemented in mock"
        ))
    }

    async fn update_table_data<E, F>(
        &self,
        _table: &crate::Table<Self, E>,
        _callback: F,
    ) -> Result<()>
    where
        E: crate::Entity,
        F: Fn(&mut E) + Send + Sync,
        Self: Sized,
    {
        Err(vantage_error!("update_table_data not implemented in mock"))
    }

    async fn delete_table_data<E>(&self, _table: &crate::Table<Self, E>) -> Result<()>
    where
        E: crate::Entity,
        Self: Sized,
    {
        Err(vantage_error!("delete_table_data not implemented in mock"))
    }

    async fn get_table_data_by_id<E>(
        &self,
        _table: &crate::Table<Self, E>,
        _id: impl vantage_dataset::dataset::Id,
    ) -> Result<E>
    where
        E: crate::Entity,
        Self: Sized,
    {
        Err(vantage_error!(
            "get_table_data_by_id not implemented in mock"
        ))
    }

    async fn insert_table_data_with_id_value<E>(
        &self,
        table: &crate::Table<Self, E>,
        id: &str,
        record: serde_json::Value,
    ) -> Result<()>
    where
        E: crate::Entity,
        Self: Sized,
    {
        let mut data = self.data.lock().unwrap();
        let vec = data
            .get_mut(&table.table_name)
            .ok_or(VantageError::no_data())?;

        // Check if ID already exists
        for value in vec.iter() {
            if let Some(record_id) = value.get("id") {
                let record_id_str = record_id
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| record_id.as_i64().map(|i| i.to_string()))
                    .or_else(|| record_id.as_u64().map(|u| u.to_string()))
                    .unwrap_or_else(|| "unknown".to_string());

                if record_id_str == id {
                    return Err(vantage_error!("Record with ID '{}' already exists", id));
                }
            }
        }

        vec.push(record);
        Ok(())
    }

    async fn replace_table_data_with_id_value<E>(
        &self,
        table: &crate::Table<Self, E>,
        id: &str,
        record: serde_json::Value,
    ) -> Result<()>
    where
        E: crate::Entity,
        Self: Sized,
    {
        let mut data = self.data.lock().unwrap();
        let vec = data
            .get_mut(&table.table_name)
            .ok_or(VantageError::no_data())?;

        // Find and replace the record with matching ID
        let mut found_index = None;
        for (index, value) in vec.iter().enumerate() {
            if let Some(record_id) = value.get("id") {
                let record_id_str = record_id
                    .as_str()
                    .map(|s| s.to_string())
                    .or_else(|| record_id.as_i64().map(|i| i.to_string()))
                    .or_else(|| record_id.as_u64().map(|u| u.to_string()))
                    .unwrap_or_else(|| "unknown".to_string());

                if record_id_str == id {
                    found_index = Some(index);
                    break;
                }
            }
        }

        if let Some(index) = found_index {
            vec[index] = record;
        } else {
            // Upsert - add if not found
            vec.push(record);
        }

        Ok(())
    }

    async fn update_table_data_value<E, F>(
        &self,
        table: &crate::Table<Self, E>,
        callback: F,
    ) -> Result<()>
    where
        E: crate::Entity,
        F: Fn(&mut serde_json::Value) + Send + Sync,
        Self: Sized,
    {
        let mut data = self.data.lock().unwrap();
        let vec = data
            .get_mut(&table.table_name)
            .ok_or(VantageError::no_data())?;

        for value in vec.iter_mut() {
            callback(value);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq, Default, Clone)]
    struct TestUser {
        id: i32,
        name: String,
    }

    #[tokio::test]
    async fn test_mock_table_source_with_data() {
        let mock = MockTableSource::new().with_data(
            "users",
            vec![
                json!({"id": 1, "name": "Alice"}),
                json!({"id": 2, "name": "Bob"}),
            ],
        );

        let table = crate::Table::new("users", mock).into_entity::<TestUser>();
        let users_with_ids: Vec<(String, TestUser)> =
            table.data_source().get_table_data(&table).await.unwrap();
        assert_eq!(users_with_ids.len(), 2);
        assert_eq!(users_with_ids[0].1.name, "Alice");
        assert_eq!(users_with_ids[1].1.name, "Bob");
    }
}
