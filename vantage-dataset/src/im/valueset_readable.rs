use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_types::Record;

use crate::{im::ImTable, traits::ReadableValueSet};

#[async_trait]
impl<E, V> ReadableValueSet for ImTable<E, V>
where
    V: Clone + Send + Sync + 'static,
    E: Send + Sync,
{
    async fn list_values(&self) -> crate::traits::Result<IndexMap<Self::Id, Record<Self::Value>>> {
        Ok(self
            .data_source
            .with_table(&self.table_name, |table| table.clone()))
    }

    async fn get_value(&self, id: &Self::Id) -> crate::traits::Result<Option<Record<Self::Value>>> {
        Ok(self
            .data_source
            .with_table(&self.table_name, |table| table.get(id).cloned()))
    }

    async fn get_some_value(
        &self,
    ) -> crate::traits::Result<Option<(Self::Id, Record<Self::Value>)>> {
        Ok(self.data_source.with_table(&self.table_name, |table| {
            table
                .iter()
                .next()
                .map(|(id, record)| (id.clone(), record.clone()))
        }))
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

        let result = table.get_value(&"nonexistent".to_string()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_some_value() {
        let ds = ImDataSource::new();
        let table = ImTable::<User>::new(&ds, "users");

        let result = table.get_some_value().await.unwrap();
        assert!(result.is_none());
    }
}
