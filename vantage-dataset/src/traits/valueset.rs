use std::pin::Pin;

use crate::record::ActiveRecord;

use super::Result;
use async_trait::async_trait;
use futures_core::Stream;
use indexmap::IndexMap;
use vantage_types::Record;

/// Foundation trait for all dataset operations, defining the basic types used for storage.
/// Typically you would implement ValueSet in combination with:
///
///  - [`ReadableValueSet`]
///  - [`WritableValueSet`]
///  - [`InsertableValueSet`]
///
/// `ValueSet` establishes the contract for working with raw storage values, providing
/// the building blocks that higher-level [`DataSet`](super::DataSet) traits build upon.
/// This separation allows the same storage backend to work with both typed entities
/// and raw values efficiently.
///
/// # Type Parameters
///
/// - `Id`: Unique identifier type chosen by the storage implementation
/// - `Value`: Raw storage value type, typically JSON-like structures
///
/// # Example
///
/// ```rust,ignore
/// use vantage_dataset::{ReadableValueSet, ValueSet, prelude::*};
/// use vantage_types::Record;
/// use serde_json::Value;
///
/// struct CsvFile {
///     filename: String,
/// }
///
/// impl ValueSet for CsvFile {
///     type Id = String;
///     type Value = serde_json::Value;
/// }
///
/// #[async_trait]
/// impl ReadableValueSet for CsvFile {
///     async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>> {
///         // Parse CSV and return as JSON values
///         // Implementation details...
///     }
///
///     async fn get_value(&self, id: &Self::Id) -> Result<Option<Record<Self::Value>>> {
///         // Find specific record by ID
///         // Implementation details...
///     }
///
///     async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>> {
///         // Return first record if any exists
///         // Implementation details...
///     }
/// }
/// ```
#[async_trait]
pub trait ValueSet {
    /// Unique identifier type for records in this storage backend.
    ///
    /// Common choices:
    /// - `String` for most databases and APIs
    /// - `uuid::Uuid` if database does not support other types of IDs.
    /// - Database-specific types like `surrealdb::sql::Thing`
    type Id: Send + Sync + Clone;

    /// Raw storage value type, representing data as stored in the backend, like
    /// serde_json::Value or cborium::Value. Can also be a custom type.
    type Value: Send + Sync + Clone;
}

/// Read-only access to raw storage values without entity deserialization.
///
/// See documentation for [`ValueSet`] for implementation example.
#[async_trait]
pub trait ReadableValueSet: ValueSet {
    /// Retrieve all records as raw storage values preserving insertion order where supported.
    ///
    /// # Performance
    /// In Vantage you can't retrieve values of a Set partially. Instead you should
    /// create a sub-set of your existing set, then list values of that set instead.
    async fn list_values(&self) -> Result<IndexMap<Self::Id, Record<Self::Value>>>;

    /// Retrieve a specific record by ID as a structured record.
    ///
    /// Returns `Ok(None)` when no record exists with the given ID. Returns an
    /// error only if the lookup itself fails.
    async fn get_value(
        &self,
        id: impl Into<Self::Id> + Send,
    ) -> Result<Option<Record<Self::Value>>>;

    /// Retrieve one single record from the set. If records are ordered - return first record.
    /// will return Ok(None).
    ///
    /// Useful when you operate with a very specific subset of data.
    async fn get_some_value(&self) -> Result<Option<(Self::Id, Record<Self::Value>)>>;

    /// Stream all records as (Id, Record) pairs.
    ///
    /// Default wraps `list_values()`. Backends with native streaming
    /// (e.g. paginated REST APIs) can override for incremental fetching.
    #[allow(clippy::type_complexity)]
    fn stream_values(
        &self,
    ) -> Pin<Box<dyn Stream<Item = Result<(Self::Id, Record<Self::Value>)>> + Send + '_>>
    where
        Self: Sync,
    {
        Box::pin(async_stream::stream! {
            let records = self.list_values().await;
            match records {
                Ok(map) => {
                    for item in map {
                        yield Ok(item);
                    }
                }
                Err(e) => yield Err(e),
            }
        })
    }
}

/// Write operations on raw storage values. **Every write is retry-safe, and a
/// write through a narrowed set only touches rows inside that set.**
///
/// "The set" is everything that narrows this value set (a table's conditions,
/// a Vista's narrowing). An unnarrowed set is the only way to write anywhere.
///
/// | operation | row in the set | row exists outside the set | no such row |
/// |---|---|---|---|
/// | `delete(id)` | deleted, `Ok(())` | untouched, `Ok(())` | `Ok(())` |
/// | `insert_value(id, r)` | untouched, returns the stored row | `Conflict` where the backend can see it; never an overwrite | inserted; must fit the set, else `Conflict` |
/// | `patch_value(id, p)` | applied; `Conflict` if the result leaves the set | `NotFound` | `NotFound` |
/// | `replace_value(id, r)` | replaced; `Conflict` if it leaves the set | `Conflict` where verifiable | created; must fit the set |
/// | `delete_all()` | deletes exactly the set | — | — |
///
/// A row "fits the set" when it satisfies every condition: equality
/// conditions are invariants (an absent or null column is filled on insert and
/// replace, validated on patch); operator conditions are evaluated by the
/// backend. A backend that can't apply or evaluate a condition refuses the
/// write with `Unsupported`. An eventually-consistent backend can't see an id
/// held outside the set before it writes; it documents what it does instead,
/// and never overwrites silently.
///
/// See documentation for [`ValueSet`] for implementation example.
#[async_trait]
pub trait WritableValueSet: ValueSet {
    /// Insert value with a specific ID (often generated) (HTTP POST with ID)
    ///
    /// **Idempotent**: an id already in the set returns the stored row
    /// untouched; an id held outside the set is `Conflict`; a new row must
    /// fit the set.
    ///
    /// **Returns**: Record as it was stored.
    ///
    /// # Use Case
    /// Generate unique ID and store record while avoiding duplicates.
    async fn insert_value(
        &self,
        id: impl Into<Self::Id> + Send,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>;

    /// Replace the entire record at the specified ID (HTTP PUT)
    ///
    /// **Idempotent**: completely overwrites existing data if present. If
    /// possible, will remove/recreate record; therefore if `record` doesn't
    /// contain certain attributes which were present in the database, those
    /// will be removed. If record does not exist, will create it.
    ///
    /// **Confined**: a row in the set is replaced (`Conflict` if the new row
    /// would leave the set); an id held outside the set is `Conflict` where
    /// the backend can verify it; a created row must fit the set.
    ///
    /// **Returns**: Record as it was stored.
    ///
    /// # Use Case
    /// Replace with a new version of a record.
    async fn replace_value(
        &self,
        id: impl Into<Self::Id> + Send,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>;

    /// Partially update a record by merging with the provided value (HTTP PATCH)
    ///
    /// **`NotFound`** (marked) when the row is missing or outside the set;
    /// `Conflict` when the patched row would leave the set. The exact merge
    /// behavior depends on the storage implementation - typically merges
    /// object fields for JSON-like values.
    ///
    /// **Returns**: Record as it was stored (not only the partial change).
    ///
    /// # Use Case
    /// Update only the modified fields of a record.
    async fn patch_value(
        &self,
        id: impl Into<Self::Id> + Send,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>;

    /// Delete a record by ID (HTTP DELETE)
    ///
    /// **Idempotent and confined**: a missing row or a row outside the set is
    /// left alone and reports `Ok(())` — nothing is revealed about rows
    /// outside the set.
    async fn delete(&self, id: impl Into<Self::Id> + Send) -> Result<()>;

    /// Delete all records in the set (HTTP DELETE without ID)
    ///
    /// **Idempotent**: deletes exactly the set. Executing several times is OK.
    ///
    /// Execute on a subset of your entire database.
    async fn delete_all(&self) -> Result<()>;
}

/// Append-only operations on raw storage values with automatic ID generation.
///
/// See documentation for [`ValueSet`] for implementation example.
#[async_trait]
pub trait InsertableValueSet: ValueSet {
    /// Insert a value and return the generated ID (Similar to HTTP POST without ID)
    ///
    /// The storage backend generates a unique identifier for the new record.
    ///
    /// # Warning
    ///
    /// This method is **not idempotent** - each call creates a new record with
    /// a new ID, even if the value data is identical.
    ///
    /// Not retry-safe: prefer `insert_value` with a client-made id (UUIDv7);
    /// use this only where the backend must make the id (declared
    /// server-made ids).
    async fn insert_return_id_value(&self, record: &Record<Self::Value>) -> Result<Self::Id>;
}

/// Change tracking for raw storage values with automatic persistence.
///
/// See documentation for [`ValueSet`] for implementation example.
#[async_trait]
pub trait ActiveRecordSet: ReadableValueSet + WritableValueSet {
    /// Retrieve a record wrapped for change tracking and deferred persistence.
    ///
    /// The returned `RecordValue` can be modified in-place and will track all
    /// changes for efficient persistence when `save()` is called.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(RecordValue))`: Record wrapper with change tracking
    /// - `Ok(None)`: No record with this ID
    /// - `Err`: If the lookup itself fails
    async fn get_value_record(
        &self,
        id: impl Into<Self::Id> + Send,
    ) -> Result<Option<ActiveRecord<'_, Self>>>;

    /// Retrieve all records wrapped for change tracking.
    ///
    /// Each returned `RecordValue` operates independently - modifications to one
    /// record don't affect others, and each must be saved separately.
    ///
    /// # Performance Note
    ///
    /// This loads all records into memory. Consider pagination or streaming
    /// approaches for large datasets.
    async fn list_value_records(&self) -> Result<Vec<ActiveRecord<'_, Self>>>;
}

// Auto-implement for any type that has both readable and writable traits
#[async_trait]
impl<T> ActiveRecordSet for T
where
    T: ReadableValueSet + WritableValueSet + Sync,
{
    async fn get_value_record(
        &self,
        id: impl Into<Self::Id> + Send,
    ) -> Result<Option<ActiveRecord<'_, Self>>> {
        let id = id.into();
        Ok(self
            .get_value(id.clone())
            .await?
            .map(|record| ActiveRecord::new(id, record, self)))
    }

    async fn list_value_records(&self) -> Result<Vec<ActiveRecord<'_, Self>>> {
        let items = self.list_values().await?;

        Ok(items
            .into_iter()
            .map(|(id, record)| ActiveRecord::new(id, record, self))
            .collect::<Vec<_>>())
    }
}
