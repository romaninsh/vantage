use std::pin::Pin;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use futures_core::Stream;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::ReadableValueSet;
use vantage_types::Record;

use crate::vista::Vista;

#[async_trait]
impl ReadableValueSet for Vista {
    async fn list_values(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        self.source().list_vista_values(self).await
    }

    async fn get_value(&self, id: &String) -> Result<Option<Record<CborValue>>> {
        self.source().get_vista_value(self, id).await
    }

    async fn get_some_value(&self) -> Result<Option<(String, Record<CborValue>)>> {
        self.source().get_vista_some_value(self).await
    }

    fn stream_values(
        &self,
    ) -> Pin<Box<dyn Stream<Item = Result<(String, Record<CborValue>)>> + Send + '_>> {
        self.source().stream_vista_values(self)
    }
}
