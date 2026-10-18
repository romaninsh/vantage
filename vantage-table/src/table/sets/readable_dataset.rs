use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use indexmap::IndexMap;

use vantage_core::Result;
use vantage_dataset::prelude::ReadableDataSet;
use vantage_types::Entity;

use crate::{table::Table, traits::table_source::TableSource};

#[async_trait]
impl<T, E> ReadableDataSet<E> for Table<T, E>
where
    T: TableSource,
    E: Entity<T::Value>,
{
    async fn list(&self) -> Result<IndexMap<Self::Id, E>> {
        self.check_read().await?;
        let records = self.data_source().list_table_values(self).await?;
        let mut entities = IndexMap::new();

        for (id, mut record) in records {
            self.apply_lazy_expressions(&mut record).await?;
            let entity: E = E::try_from_record(&record)
                .map_err(|_| vantage_core::error!("Failed to convert record to entity"))?;
            entities.insert(id, entity);
        }

        Ok(entities)
    }

    async fn get(&self, id: impl Into<Self::Id> + Send) -> Result<Option<E>> {
        self.check_read().await?;
        let id = id.into();
        let Some(mut record) = self.data_source().get_table_value(self, &id).await? else {
            return Ok(None);
        };
        self.apply_lazy_expressions(&mut record).await?;
        let entity = E::try_from_record(&record)
            .map_err(|_| vantage_core::error!("Failed to convert record to entity"))?;
        Ok(Some(entity))
    }

    async fn get_some(&self) -> Result<Option<(Self::Id, E)>> {
        self.check_read().await?;
        if let Some((id, mut record)) = self.data_source().get_table_some_value(self).await? {
            self.apply_lazy_expressions(&mut record).await?;
            let entity: E = E::try_from_record(&record)
                .map_err(|_| vantage_core::error!("Failed to convert record to entity"))?;
            Ok(Some((id, entity)))
        } else {
            Ok(None)
        }
    }
}

impl<T, E> Table<T, E>
where
    T: TableSource,
    E: Entity<T::Value>,
{
    /// Stream all entities as (Id, Entity) pairs.
    ///
    /// Delegates to `stream_table_values` on the data source, converting
    /// each record to the entity type. Backends with native streaming
    /// (e.g. paginated REST APIs) yield records incrementally.
    #[allow(clippy::type_complexity)]
    pub fn stream(&self) -> Pin<Box<dyn Stream<Item = Result<(T::Id, E)>> + Send + '_>> {
        let value_stream = self.data_source().stream_table_values(self);
        Box::pin(async_stream::stream! {
            if let Err(e) = self.check_read().await {
                yield Err(e);
                return;
            }
            tokio::pin!(value_stream);
            loop {
                let item = {
                    use std::future::poll_fn;
                    use std::task::Poll;
                    poll_fn(|cx| match value_stream.as_mut().poll_next(cx) {
                        Poll::Ready(item) => Poll::Ready(item),
                        Poll::Pending => Poll::Pending,
                    }).await
                };
                let mapped: Option<Result<(T::Id, E)>> = match item {
                    Some(Ok((id, record))) => {
                        // Convert record to entity — handle error before yield to keep Send
                        let result = E::try_from_record(&record)
                            .map(|entity| (id, entity))
                            .map_err(|_| vantage_core::error!("Failed to convert record to entity"));
                        Some(result)
                    }
                    Some(Err(e)) => Some(Err(e)),
                    None => None,
                };
                match mapped {
                    Some(item) => yield item,
                    None => break,
                }
            }
        })
    }
}
