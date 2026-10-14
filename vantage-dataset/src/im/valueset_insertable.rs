use async_trait::async_trait;
use vantage_types::Record;

use crate::{im::ImTable, traits::InsertableValueSet};

/// `InsertableValueSet` is only available for `V = serde_json::Value` because
/// it inspects the record for an `"id"` field via JSON-specific accessors
/// (`as_str`, `as_u64`, …). CBOR consumers carry their own id generation
/// upstream (e.g. `MockTableSource::insert_table_value` constructs the id
/// before calling into `ImTable`).
#[async_trait]
impl<E> InsertableValueSet for ImTable<E, serde_json::Value>
where
    E: Send + Sync,
{
    async fn insert_return_id_value(
        &self,
        record: &Record<Self::Value>,
    ) -> crate::traits::Result<Self::Id> {
        let id = if let Some(record_id) = record.get("id") {
            if record_id.is_null() {
                self.generate_id()
            } else if let Some(id_str) = record_id.as_str() {
                if id_str.is_empty() {
                    self.generate_id()
                } else {
                    id_str.to_string()
                }
            } else if let Some(id_num) = record_id.as_u64() {
                id_num.to_string()
            } else {
                self.generate_id()
            }
        } else {
            self.generate_id()
        };

        self.data_source.with_table_mut(&self.table_name, |table| {
            table.insert(id.clone(), record.clone())
        });

        Ok(id)
    }
}
