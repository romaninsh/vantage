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
        let mut rows = self.source.list_vista_values(self).await?;
        self.fill_computed_rows(rows.values_mut())?;
        Ok(rows)
    }

    async fn get_value(&self, id: impl Into<String> + Send) -> Result<Option<Record<CborValue>>> {
        let id = id.into();
        let mut row = self.source.get_vista_value(self, &id).await?;
        self.fill_computed_rows(row.as_mut())?;
        Ok(row)
    }

    async fn get_some_value(&self) -> Result<Option<(String, Record<CborValue>)>> {
        let mut row = self.source.get_vista_some_value(self).await?;
        self.fill_computed_rows(row.as_mut().map(|(_, r)| r))?;
        Ok(row)
    }

    fn stream_values(
        &self,
    ) -> Pin<Box<dyn Stream<Item = Result<(String, Record<CborValue>)>> + Send + '_>> {
        let mut inner = self.source.stream_vista_values(self);
        if !self.has_computed() {
            return inner;
        }
        Box::pin(async_stream::stream! {
            while let Some(item) = std::future::poll_fn(|cx| inner.as_mut().poll_next(cx)).await {
                yield item.and_then(|(id, mut row)| {
                    self.fill_computed(&mut row)?;
                    Ok((id, row))
                });
            }
        })
    }
}
