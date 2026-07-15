use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_types::{Entity, Record};

use crate::{table::Table, traits::table_source::TableSource};

// Implement ReadableValueSet by delegating to data source, then applying
// any lazy expressions to the returned records (see
// `Table::with_lazy_expression`).
#[async_trait]
impl<T: TableSource, E: Entity<T::Value>> ReadableValueSet for Table<T, E> {
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>> {
        let mut rows = self.data_source().list_table_values(self).await?;
        for (_, record) in rows.iter_mut() {
            self.apply_lazy_expressions(record).await?;
        }
        Ok(rows)
    }

    async fn get_value(
        &self,
        id: impl Into<Self::Id> + Send,
    ) -> Result<Option<Record<Self::Value>>> {
        let id = id.into();
        let Some(mut record) = self.data_source().get_table_value(self, &id).await? else {
            return Ok(None);
        };
        self.apply_lazy_expressions(&mut record).await?;
        Ok(Some(record))
    }

    async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>> {
        let Some((id, mut record)) = self.data_source().get_table_some_value(self).await? else {
            return Ok(None);
        };
        self.apply_lazy_expressions(&mut record).await?;
        Ok(Some((id, record)))
    }

    fn stream_values(
        &self,
    ) -> Pin<Box<dyn Stream<Item = Result<(Self::Id, Record<Self::Value>)>> + Send + '_>> {
        let mut source = self.data_source().stream_table_values(self);
        if self.lazy_expressions.is_empty() {
            return source;
        }
        // Streamed records carry the same computed columns as list/get reads.
        Box::pin(async_stream::stream! {
            while let Some(item) =
                std::future::poll_fn(|cx| source.as_mut().poll_next(cx)).await
            {
                match item {
                    Ok((id, mut record)) => match self.apply_lazy_expressions(&mut record).await {
                        Ok(()) => yield Ok((id, record)),
                        Err(e) => yield Err(e),
                    },
                    Err(e) => yield Err(e),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_table_source::MockTableSource;
    use serde_json::json;
    use vantage_types::EmptyEntity;

    #[tokio::test]
    async fn lazy_expressions_chain_in_declaration_order_and_register_columns() {
        let mock_source = MockTableSource::new()
            .with_data("t", vec![json!({"id": "1", "name": "Alice"})])
            .await;
        let table = Table::<MockTableSource, EmptyEntity>::new("t", mock_source)
            .with_lazy_expression("greeting", |r| {
                // Clone out of the borrowed record before going async.
                let name = r
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                async move { Ok(json!(format!("hello {name}"))) }
            })
            .with_lazy_expression("shout", |r| {
                // A later lazy expression sees the earlier one's column.
                let greeting = r
                    .get("greeting")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                async move { Ok(json!(greeting.to_uppercase())) }
            });

        assert!(table.columns().contains_key("greeting"));
        assert!(table.columns().contains_key("shout"));

        let rows = table.list_values().await.unwrap();
        assert_eq!(rows["1"]["greeting"], json!("hello Alice"));
        assert_eq!(rows["1"]["shout"], json!("HELLO ALICE"));

        let row = table.get_value("1").await.unwrap().expect("row 1");
        assert_eq!(row["shout"], json!("HELLO ALICE"));

        // Streaming reads carry the same computed columns.
        let mut stream = table.stream_values();
        let mut streamed = Vec::new();
        while let Some(item) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            streamed.push(item.unwrap());
        }
        assert_eq!(streamed.len(), 1);
        assert_eq!(streamed[0].1["shout"], json!("HELLO ALICE"));
    }

    #[tokio::test]
    async fn test_readable_value_set_implementation() {
        // Setup mock data
        let mock_data = vec![
            json!({"id": "1", "name": "Alice", "age": 30}),
            json!({"id": "2", "name": "Bob", "age": 25}),
            json!({"id": "3", "name": "Charlie", "age": 35}),
        ];

        let mock_source = MockTableSource::new()
            .with_data("test_table", mock_data)
            .await;
        let table = Table::<MockTableSource, EmptyEntity>::new("test_table", mock_source);

        // Test list_values
        let all_values = table.list_values().await.unwrap();
        assert_eq!(all_values.len(), 3);

        // Check that we have the expected IDs
        assert!(all_values.contains_key("1"));
        assert!(all_values.contains_key("2"));
        assert!(all_values.contains_key("3"));

        // Check that records contain expected data
        let record_1 = &all_values["1"];
        assert_eq!(record_1["name"], json!("Alice"));
        assert_eq!(record_1["age"], json!(30));

        // Test get_value with existing ID
        let value_2 = table.get_value("2").await.unwrap().expect("row 2");
        assert_eq!(value_2["name"], json!("Bob"));
        assert_eq!(value_2["age"], json!(25));

        // Test get_value with non-existing ID
        let result = table.get_value("999").await.unwrap();
        assert!(result.is_none());

        // Test get_some_value
        let some_value = table.get_some_value().await.unwrap();
        assert!(some_value.is_some());

        let (id, record) = some_value.unwrap();
        assert_eq!(id, "1"); // Should be the first record
        assert_eq!(record["name"], json!("Alice"));

        // Test get_some_value with empty table
        let empty_source = MockTableSource::new()
            .with_data("empty_table", vec![])
            .await;
        let empty_table = Table::<MockTableSource, EmptyEntity>::new("empty_table", empty_source);
        let empty_result = empty_table.get_some_value().await.unwrap();
        assert!(empty_result.is_none());
    }
}
