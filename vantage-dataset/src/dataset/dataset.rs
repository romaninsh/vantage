use crate::{dataset::ValueSet, record::Record};

use super::Result;
use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::Entity;

/// Implement ReadableDataSet / WritableDataSet depending on the capabilities
/// of data source. Alternatively you can implement InsertableDataSet, that
/// supports Id generation on insert.
#[async_trait]
pub trait DataSet<E: Entity>: ValueSet {}

#[async_trait]
pub trait ReadableDataSet<E: Entity>: DataSet<E> {
    /// Get all items from DataSet and return them as Id=>Entity
    async fn list(&self) -> Result<IndexMap<Self::Id, E>>;

    /// Get specific record with &id, returing it as Entity
    async fn get(&self, id: &Self::Id) -> Result<E>;

    /// Get one Entity along with id or return None
    async fn get_some(&self) -> Result<Option<(Self::Id, E)>>;
}

#[async_trait]
pub trait WritableDataSet<E: Entity>: DataSet<E> {
    /// Insert a record with a specific ID using Value. Can be retried with the same
    /// Id if fails. If Id already exist - will do nothing and return success.
    /// (see documentation on Idemnpotence)
    async fn insert(&self, id: &Self::Id, record: E) -> Result<()>;

    /// Replace a record of specified Id using Value. If executed multiple time,
    /// will be successful.
    async fn replace(&self, id: &Self::Id, record: E) -> Result<()>;

    /// Partially update a record by ID, fails if record doesn't exist. Multiple
    /// execution will be successful.
    async fn patch(&self, id: &Self::Id, partial: E) -> Result<()>;
}

#[async_trait]
pub trait InsertableDataSet<E: Entity>: DataSet<E> {
    /// Insert a record and return generated ID. It's not advisable to use this method, due to
    /// idemnpotence issues.
    async fn insert_return_id(&self, record: E) -> Result<Self::Id>;
}

/// Extension to DataSet, which allow you to return Entity wrapped in a Record. This will
/// permit user to modify entity (through mutable deref), then call save() to
/// store modifications into database
#[async_trait]
pub trait RecordDataSet<E>: ReadableDataSet<E> + WritableDataSet<E>
where
    E: Entity,
{
    async fn get_record(&self, id: &Self::Id) -> Result<Option<Record<'_, Self, E>>> {
        match self.get(id).await {
            Ok(data) => Ok(Some(Record::new(id.clone(), data, self))),
            Err(_) => Ok(None),
        }
    }

    async fn list_records(&self) -> Result<Vec<Record<'_, Self, E>>> {
        let items = self.list().await?;

        Ok(items
            .into_iter()
            .map(|(id, data)| Record::new(id, data, self))
            .collect::<Vec<_>>())
    }
}
// Auto-implement for any type that has both readable and writable traits
impl<T, E> RecordDataSet<E> for T
where
    T: ReadableDataSet<E> + WritableDataSet<E>,
    E: Entity,
{
}

// // Auto-implement for any type that has both readable and writable traits
// #[async_trait]
// impl<T> RecordValueSet for T
// where
//     T: ReadableValueSet + WritableValueSet,
//     Self::Value: Send + Sync + Clone,
// {
//     async fn get_value_record(&self, id: &Self::Id) -> Result<RecordValue<'_, Self>> {
//         let value = self.get_value(id).await?;
//         Ok(RecordValue::new(id, value, self))
//     }

//     async fn list_value_records(&self) -> Result<Vec<RecordValue<'_, Self>>> {
//         let items = self.list_values().await?;

//         Ok(items
//             .into_iter()
//             .map(|(id, value)| RecordValue::new(id, value, self))
//             .collect::<Vec<_>>())
//     }
// }
