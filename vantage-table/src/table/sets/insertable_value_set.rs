use async_trait::async_trait;
use vantage_core::Result;
use vantage_dataset::prelude::InsertableValueSet;
use vantage_types::{Entity, InvariantValue, Record};

use crate::{prelude::TableSource, table::Table};

// The typed-entity path (insertable_dataset) funnels through here, so hooks and
// invariants run in one place.
#[async_trait]
impl<T: TableSource, E: Entity<T::Value>> InsertableValueSet for Table<T, E>
where
    T::Value: InvariantValue,
{
    async fn insert_return_id_value(&self, record: &Record<Self::Value>) -> Result<Self::Id> {
        let record = self.prepare_insert(record).await?;
        let id = self
            .data_source()
            .insert_table_return_id_value(self, &record)
            .await?;
        self.finish_insert(&id, &record).await?;
        Ok(id)
    }
}
