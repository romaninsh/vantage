use async_trait::async_trait;
use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::WritableValueSet;
use vantage_types::Record;

use crate::vista::Vista;

#[async_trait]
impl WritableValueSet for Vista {
    async fn insert_value(
        &self,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.source().insert_vista_value(self, id, record).await
    }

    async fn replace_value(
        &self,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.source().replace_vista_value(self, id, record).await
    }

    async fn patch_value(
        &self,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.source().patch_vista_value(self, id, partial).await
    }

    async fn delete(&self, id: &String) -> Result<()> {
        self.source().delete_vista_value(self, id).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.source().delete_vista_all_values(self).await
    }
}
