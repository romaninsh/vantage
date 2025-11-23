use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_types::{Entity, Record};

use crate::{im::ImTable, traits::ReadableValueSet};

#[async_trait]
impl<E> ReadableValueSet for ImTable<E>
where
    E: Entity,
{
    async fn list_values(&self) -> crate::traits::Result<IndexMap<Self::Id, Record<Self::Value>>> {
        let table = self.data_source.get_or_create_table(&self.table_name);
        Ok(table)
    }

    async fn get_value(&self, id: &Self::Id) -> crate::traits::Result<Record<Self::Value>> {
        let table = self.data_source.get_or_create_table(&self.table_name);

        match table.get(id) {
            Some(record) => Ok(record.clone()),
            None => Err(vantage_core::util::error::vantage_error!(
                "Record with id '{}' not found",
                id
            )),
        }
    }

    async fn get_some_value(
        &self,
    ) -> crate::traits::Result<Option<(Self::Id, Record<Self::Value>)>> {
        let table = self.data_source.get_or_create_table(&self.table_name);

        if let Some((id, record)) = table.iter().next() {
            Ok(Some((id.clone(), record.clone())))
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
