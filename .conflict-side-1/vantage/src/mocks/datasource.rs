use std::{ops::Deref, sync::Arc};

use crate::sql::Query;
use crate::traits::datasource::DataSource;
use anyhow::Result;
use serde_json::{Map, Value};

#[derive(Clone, Debug)]
pub struct MockDataSource {
    data: Arc<Vec<Map<String, Value>>>,
}

impl MockDataSource {
    pub fn new(data: &Value) -> MockDataSource {
        let data = data
            .as_array()
            .unwrap()
            .clone()
            .into_iter()
            .map(|x| x.as_object().unwrap().clone())
            .collect();
        MockDataSource {
            data: Arc::new(data),
        }
    }

    pub fn data(&self) -> &Vec<Map<String, Value>> {
        &self.data
    }
}

impl DataSource for MockDataSource {
    async fn query_fetch(&self, _query: &Query) -> Result<Vec<Map<String, Value>>> {
        Ok(self.data.deref().clone())
    }

    async fn query_exec(&self, _query: &Query) -> Result<Option<Value>> {
        Ok(None)
    }

    async fn query_insert(
        &self,
        _query: &Query,
        _rows: Vec<Vec<serde_json::Value>>,
    ) -> anyhow::Result<()> {
        todo!()
    }

    async fn query_one(&self, _query: &Query) -> Result<Value> {
        todo!()
    }
    async fn query_row(&self, query: &Query) -> Result<Map<String, Value>> {
        todo!()
    }
    async fn query_col(&self, query: &Query) -> Result<Vec<Value>> {
        todo!()
    }
}

impl PartialEq for MockDataSource {
    fn eq(&self, other: &MockDataSource) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::Query;
    use crate::traits::datasource::DataSource;
    use serde_json::json;
    use tokio;

    #[tokio::test]
    async fn test_mock_data_source() {
        let json = json!([{
            "name": "John",
            "surname": "Doe"
        },
        {
            "name": "Jane",
            "surname": "Doe"
        }
        ]);

        let data_source = MockDataSource::new(&json);

        let query = Query::new()
            .with_table("users", None)
            .with_column_field("name")
            .with_column_field("surname");
        let result = data_source.query_fetch(&query).await;

        assert_eq!(result.unwrap(), *data_source.data());
    }
}
