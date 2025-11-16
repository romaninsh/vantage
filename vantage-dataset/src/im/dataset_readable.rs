use async_trait::async_trait;
use indexmap::IndexMap;

use vantage_types::{Entity, Record};

use crate::{
    im::ImTable,
    traits::{DataSet, ReadableDataSet, Result},
};
use vantage_core::util::error::vantage_error;

#[async_trait]
impl<E> DataSet<E> for ImTable<E> where E: Entity {}

#[async_trait]
impl<E> ReadableDataSet<E> for ImTable<E>
where
    E: Entity,
    <E as TryFrom<Record<serde_json::Value>>>::Error: std::fmt::Debug,
{
    async fn list(&self) -> Result<IndexMap<Self::Id, E>> {
        let table = self.data_source.get_or_create_table(&self.table_name);
        let mut records = IndexMap::new();

        for (id, record) in table.iter() {
            // Add the id field to the record for conversion
            let mut record_with_id = record.clone();
            record_with_id.insert("id".to_string(), serde_json::Value::String(id.clone()));

            let entity: E = E::try_from(record_with_id)
                .map_err(|e| vantage_error!("Failed to convert record to entity: {:?}", e))?;
            records.insert(id.clone(), entity);
        }

        Ok(records)
    }

    async fn get(&self, id: &Self::Id) -> Result<E> {
        let table = self.data_source.get_or_create_table(&self.table_name);

        match table.get(id) {
            Some(record) => {
                // Add the id field to the record for conversion
                let mut record_with_id = record.clone();
                record_with_id.insert("id".to_string(), serde_json::Value::String(id.clone()));

                E::try_from(record_with_id)
                    .map_err(|e| vantage_error!("Failed to convert record to entity: {:?}", e))
            }
            None => Err(vantage_error!("Record with id '{}' not found", id)),
        }
    }

    async fn get_some(&self) -> Result<Option<(Self::Id, E)>> {
        let table = self.data_source.get_or_create_table(&self.table_name);

        if let Some((id, record)) = table.iter().next() {
            // Add the id field to the record for conversion
            let mut record_with_id = record.clone();
            record_with_id.insert("id".to_string(), serde_json::Value::String(id.clone()));

            let entity: E = E::try_from(record_with_id)
                .map_err(|e| vantage_error!("Failed to convert record to entity: {:?}", e))?;
            Ok(Some((id.clone(), entity)))
        } else {
            Ok(None)
        }
    }
}

// Tests are in tests/im_dataset.rs integration tests
