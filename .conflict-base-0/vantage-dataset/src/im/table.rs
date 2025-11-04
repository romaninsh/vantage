use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

use crate::{
    dataset::{ReadableDataSet, Result},
    im::ImDataSource,
};
use vantage_core::util::error::Context;

/// Table represents a typed table in the ImDataSource
pub struct ImTable<E> {
    pub(super) data_source: ImDataSource,
    pub(super) table_name: String,
    _phantom: std::marker::PhantomData<E>,
}

impl<E> ImTable<E>
where
    E: Serialize + DeserializeOwned,
{
    pub fn new(data_source: &ImDataSource, table_name: &str) -> Self {
        Self {
            data_source: data_source.clone(),
            table_name: table_name.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn generate_id(&self) -> String {
        Uuid::new_v4().to_string()
    }

    pub async fn import(&mut self, ds: impl ReadableDataSet<E>) -> Result<()> {
        let mut table = self.data_source.get_or_create_table(&self.table_name);
        table.clear();

        for i in ds.get().await?.into_iter() {
            let mut value =
                serde_json::to_value(i).context("Failed to serialize record during import")?;

            // Extract ID from record if present, otherwise generate random ID
            let id = if let Some(record_id) = value.get("id") {
                if record_id.is_null() {
                    self.generate_id()
                } else if let Some(id_str) = record_id.as_str() {
                    id_str.to_string()
                } else if let Some(id_num) = record_id.as_u64() {
                    id_num.to_string()
                } else {
                    self.generate_id()
                }
            } else {
                self.generate_id()
            };

            // Remove id from the stored record since it's in the key
            if let serde_json::Value::Object(ref mut map) = value {
                map.remove("id");
            }

            table.insert(id, value);
        }

        self.data_source.update_table(&self.table_name, table);
        Ok(())
    }
}
