use super::MockColumn;
use crate::{TableLike, TableSource};
use async_trait::async_trait;
use std::collections::HashMap;
use vantage_expressions::protocol::datasource::DataSource;

pub struct MockTableSource {
    data: HashMap<String, Vec<serde_json::Value>>,
}

impl MockTableSource {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    pub fn with_data(mut self, table_name: &str, data: Vec<serde_json::Value>) -> Self {
        self.data.insert(table_name.to_string(), data);
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

    async fn get_table_data_as<E>(
        &self,
        table: &crate::Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<E>>
    where
        E: crate::Entity,
        Self: Sized,
    {
        let values = self.get_table_data_values(table).await?;
        let mut results = Vec::new();

        for value in values {
            match serde_json::from_value::<E>(value) {
                Ok(item) => results.push(item),
                Err(e) => {
                    return Err(vantage_dataset::dataset::DataSetError::other(e.to_string()));
                }
            }
        }

        Ok(results)
    }

    async fn get_table_data_some_as<E>(
        &self,
        table: &crate::Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Option<E>>
    where
        E: crate::Entity,
        Self: Sized,
    {
        let values = self.get_table_data_values(table).await?;

        if let Some(first_value) = values.into_iter().next() {
            match serde_json::from_value::<E>(first_value) {
                Ok(item) => Ok(Some(item)),
                Err(e) => Err(vantage_dataset::dataset::DataSetError::other(e.to_string())),
            }
        } else {
            Ok(None)
        }
    }

    async fn get_table_data_values<E>(
        &self,
        _table: &crate::Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<serde_json::Value>>
    where
        E: crate::Entity,
        Self: Sized,
    {
        match self.data.get(&_table.table_name) {
            Some(data) => Ok(data.clone()),
            None => Ok(vec![]),
        }
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
        let users: Vec<TestUser> = table.data_source().get_table_data_as(&table).await.unwrap();
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "Alice");
        assert_eq!(users[1].name, "Bob");
    }
}
