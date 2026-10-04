use async_trait::async_trait;
use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::WritableValueSet;
use vantage_types::Record;

use crate::vista::Vista;

/// Every write drops the record's computed columns before the backend sees it.
#[async_trait]
impl WritableValueSet for Vista {
    async fn insert_value(
        &self,
        id: impl Into<String> + Send,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let id = id.into();
        self.insert_nested_value(&id, record).await
    }

    async fn replace_value(
        &self,
        id: impl Into<String> + Send,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let id = id.into();
        let record = self.without_computed(record);
        self.source.replace_vista_value(self, &id, &record).await
    }

    async fn patch_value(
        &self,
        id: impl Into<String> + Send,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let id = id.into();
        let partial = self.without_computed(partial);
        self.source.patch_vista_value(self, &id, &partial).await
    }

    async fn delete(&self, id: impl Into<String> + Send) -> Result<()> {
        let id = id.into();
        self.source.delete_vista_value(self, &id).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.source.delete_vista_all_values(self).await
    }
}

impl Vista {
    /// Insert `id`, or replace it if it exists.
    ///
    /// Inherent rather than on [`WritableValueSet`] — that trait is shared
    /// with `ImTable` and `Table<T, E>`, and `upsert` is a `Vista`-only verb.
    pub async fn upsert_value(
        &self,
        id: impl Into<String> + Send,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let id = id.into();
        let record = self.without_computed(record);
        self.source.upsert_vista_value(self, &id, &record).await
    }
}
