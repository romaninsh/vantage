use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_types::{Entity, Record};

use crate::{table::Table, traits::table_source::TableSource};

#[async_trait]
impl<T: TableSource, E: Entity<T::Value>> ReadableValueSet for Table<T, E> {
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>> {
        let mut rows = self.data_source().list_table_values(self).await?;
        for (_, record) in rows.iter_mut() {
            self.apply_lazy_expressions(record).await?;
        }
        Ok(rows)
    }

    async fn get_value(
        &self,
        id: impl Into<Self::Id> + Send,
    ) -> Result<Option<Record<Self::Value>>> {
        let id = id.into();
        let Some(mut record) = self.data_source().get_table_value(self, &id).await? else {
            return Ok(None);
        };
        self.apply_lazy_expressions(&mut record).await?;
        Ok(Some(record))
    }

    async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>> {
        let Some((id, mut record)) = self.data_source().get_table_some_value(self).await? else {
            return Ok(None);
        };
        self.apply_lazy_expressions(&mut record).await?;
        Ok(Some((id, record)))
    }

    fn stream_values(
        &self,
    ) -> Pin<Box<dyn Stream<Item = Result<(Self::Id, Record<Self::Value>)>> + Send + '_>> {
        let mut source = self.data_source().stream_table_values(self);
        if self.lazy_expressions.is_empty() {
            return source;
        }
        Box::pin(async_stream::stream! {
            while let Some(item) =
                std::future::poll_fn(|cx| source.as_mut().poll_next(cx)).await
            {
                match item {
                    Ok((id, mut record)) => match self.apply_lazy_expressions(&mut record).await {
                        Ok(()) => yield Ok((id, record)),
                        Err(e) => yield Err(e),
                    },
                    Err(e) => yield Err(e),
                }
            }
        })
    }
}
