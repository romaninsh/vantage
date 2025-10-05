use crate::SurrealDB;
use async_trait::async_trait;
use vantage_core::util::error::Context;
use vantage_expressions::Expression;
use vantage_table::Table;

#[async_trait]
impl vantage_table::TableSource for SurrealDB {
    type Column = crate::SurrealColumn;
    type Expr = Expression;

    fn create_column(&self, name: &str, _table: impl vantage_table::TableLike) -> Self::Column {
        crate::SurrealColumn::new(name)
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<vantage_expressions::protocol::expressive::IntoExpressive<Self::Expr>>,
    ) -> Self::Expr {
        Expression::new(template, parameters)
    }

    async fn get_table_data<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<E>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let select = table.select();
        let raw_result = select.get(self).await;

        let entities = raw_result
            .into_iter()
            .map(|item| serde_json::from_value(serde_json::Value::Object(item)))
            .collect::<std::result::Result<Vec<E>, _>>()
            .context("Failed to deserialize entities")?;

        Ok(entities)
    }

    async fn get_table_data_some<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Option<E>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let select = table.select().only_first_row();
        let raw_result = select.get(self).await;

        let entity = serde_json::from_value(serde_json::Value::Object(raw_result))
            .context("Failed to deserialize entity")?;

        Ok(Some(entity))
    }

    async fn get_table_data_as_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<serde_json::Value>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let select = table.select();
        let raw_result = select.get(self).await;

        let values = raw_result
            .into_iter()
            .map(serde_json::Value::Object)
            .collect();

        Ok(values)
    }

    async fn insert_table_data<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        record: E,
    ) -> vantage_dataset::dataset::Result<Option<String>>
    where
        E: vantage_core::Entity + serde::Serialize,
        Self: Sized,
    {
        let data = serde_json::to_value(record).context("Failed to serialize record")?;

        let table_obj = surreal_client::Table::new(table.table_name());
        let client = self.inner.lock().await;
        let result = client
            .insert(&table_obj.to_string(), data)
            .await
            .context("Failed to insert record")?;

        // Extract ID from result - SurrealDB typically returns the inserted record with ID
        if let Some(obj) = result.as_object()
            && let Some(id_val) = obj.get("id")
            && let Some(id_str) = id_val.as_str()
        {
            return Ok(Some(id_str.to_string()));
        }

        Ok(None)
    }
}
