use async_trait::async_trait;
use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::InsertableValueSet;
use vantage_types::Record;

use crate::vista::Vista;

#[async_trait]
impl InsertableValueSet for Vista {
    async fn insert_return_id_value(&self, record: &Record<CborValue>) -> Result<String> {
        self.source()
            .insert_vista_return_id_value(self, record)
            .await
    }
}
