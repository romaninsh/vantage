use crate::record::RecordValue;

use super::Result;
use async_trait::async_trait;
use indexmap::IndexMap;

/// A ValueSet is a collection of JSON values identified by unique IDs, that can be
/// retrieved and updated individually.
#[async_trait]
pub trait ValueSet {
    /// Trait implementation may choose type to use for Id field. Normally
    /// this is a String, although can be UUID or Thing.
    type Id: Send + Sync + Clone;

    /// Trait implementation may choose type to use to describe a record.
    /// Normally that's Value (using Map variant). Do not confuse this with
    /// Entity.
    type Value: Send + Sync + Clone;
}

#[async_trait]
pub trait ReadableValueSet: ValueSet {
    /// Get all items from ValueSet as-they-are and return them as Id=>Value
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Self::Value>>;

    /// Get specific record with &id, returing it as Value or fail
    async fn get_value(&self, id: &Self::Id) -> Result<Self::Value>;

    /// Get one record along with id or return None
    async fn get_some_value(&self) -> Result<Option<(Self::Id, Self::Value)>>;
}

#[async_trait]
pub trait WritableValueSet: ValueSet {
    /// Insert a record with a specific ID using Value. Can be retried with the same
    /// Id if fails. If Id already exist - will do nothing and return success.
    /// (see documentation on Idemnpotence)
    async fn insert_value(&self, id: &Self::Id, record: Self::Value) -> Result<()>;

    /// Replace a record of specified Id using Value. If executed multiple time,
    /// will be successful.
    async fn replace_value(&self, id: &Self::Id, record: Self::Value) -> Result<()>;

    /// Partially update a record by ID, fails if record doesn't exist. Multiple
    /// execution will be successful.
    async fn patch_value(&self, id: &Self::Id, partial: Self::Value) -> Result<()>;

    /// Delete a record by Id. If record does not exist, will still return success.
    async fn delete(&self, id: &Self::Id) -> Result<()>;

    /// Delete all records in the DataSet
    async fn delete_all(&self) -> Result<()>;
}

/// Extension to ValueSet, which allow you to return Value wrapped in a Record. This will
/// permit user to modify record value (through mutable deref), then call save() to
/// patch modifications back into database
#[async_trait]
pub trait RecordValueSet: ReadableValueSet + WritableValueSet {
    /// Retrieve one record, described through a Value from the database and wrap into
    /// Record. That means you can modify Value through mutable deref, then call save()
    /// to patch modifications back into data source.
    async fn get_value_record(&self, id: &Self::Id) -> Result<RecordValue<'_, Self>>;

    /// Retrive all records, described through a Value and wrapped in a Record. You can
    /// drop or call save() to store your modifications back into data source.
    async fn list_value_records(&self) -> Result<Vec<RecordValue<'_, Self>>>;
}

// Auto-implement for any type that has both readable and writable traits
#[async_trait]
impl<T> RecordValueSet for T
where
    T: ReadableValueSet + WritableValueSet + Sync,
{
    async fn get_value_record(&self, id: &Self::Id) -> Result<RecordValue<'_, Self>> {
        let value = self.get_value(id).await?;
        Ok(RecordValue::new(id.clone(), value, self))
    }

    async fn list_value_records(&self) -> Result<Vec<RecordValue<'_, Self>>> {
        let items = self.list_values().await?;

        Ok(items
            .into_iter()
            .map(|(id, value)| RecordValue::new(id, value, self))
            .collect::<Vec<_>>())
    }
}
