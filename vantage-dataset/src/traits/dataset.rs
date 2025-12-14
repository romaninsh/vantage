use crate::{ActiveEntity, traits::ValueSet};

use super::Result;
use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_types::Entity;

/// Entity-aware dataset operations built on top of the [`ValueSet`] foundation.
///
/// `DataSet` bridges the gap between raw storage values and typed Rust entities,
/// providing automatic serialization/deserialization while preserving the flexibility
/// of the underlying storage backend.
///
/// # Type Parameters
///
/// - `E`: The entity type that implements [`Entity`] trait, typically your domain models
///
/// # Relationship to ValueSet
///
/// While [`ValueSet`] works with raw storage values (JSON, CBOR, etc.), `DataSet`
/// adds a typed layer that handles conversion between entities and storage format:
///
/// ```text
/// Entity <--serde--> Value <--storage--> Backend
/// ```
///
/// This separation allows the same storage backend to efficiently support both
/// raw value operations and typed entity operations as needed.
///
/// # Implementation Strategy
///
/// Implement the specific capability traits your data source supports:
/// - [`ReadableDataSet`] for read-only sources (CSV files, APIs)
/// - [`InsertableDataSet`] for append-only sources (message queues, logs)
/// - [`WritableDataSet`] for full CRUD sources (databases, caches)
/// - [`entityDataSet`] for change-tracking scenarios (interactive applications)
///
/// # Example
///
/// ```rust,ignore
/// use vantage_dataset::dataset::{DataSet, ReadableDataSet, WritableDataSet};
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize, Clone)]
/// struct User {
///     name: String,
///     email: String,
///     age: u32,
/// }
///
/// // Your storage implementation
/// struct UserTable;
///
/// impl ValueSet for UserTable {
///     type Id = String;
///     type Value = serde_json::Value;
/// }
///
/// impl DataSet<User> for UserTable {}
///
/// impl ReadableDataSet<User> for UserTable {
///     async fn list(&self) -> Result<IndexMap<String, User>> {
///         // Implementation converts storage values to entities
///     }
/// }
/// ```
#[async_trait]
pub trait DataSet<E>: ValueSet
where
    E: Entity<Self::Value>,
{
}

/// Read-only access to typed entities with automatic deserialization.
///
/// This trait provides convenient access to entities without requiring knowledge
/// of the underlying storage format. The implementation handles conversion from
/// raw storage values to typed entities automatically.
///
/// # Performance Considerations
///
/// Entity deserialization has overhead compared to raw value access. For
/// performance-critical scenarios, consider using [`crate::ReadableValueSet`] directly
/// and handling deserialization manually or in batches.
///
/// # Example
///
/// ```rust,ignore
/// use vantage_dataset::dataset::ReadableDataSet;
///
/// // Type-safe entity access
/// let all_users: IndexMap<String, User> = users.list().await?;
/// let specific_user: User = users.get(&user_id).await?;
///
/// // Sample data without loading everything
/// if let Some((id, user)) = users.get_some().await? {
///     println!("Found user: {} with ID {}", user.name, id);
/// }
/// ```
#[async_trait]
pub trait ReadableDataSet<E>: DataSet<E>
where
    E: Entity<Self::Value>,
{
    /// Retrieve all entities with their IDs.
    ///
    /// Returns an ordered map preserving insertion order where supported by the backend.
    /// Each storage value is automatically deserialized to the entity type.
    ///
    /// # Performance Warning
    ///
    /// This method loads all data into memory. Use with caution for large datasets.
    /// Consider implementing pagination for production use.
    async fn list(&self) -> Result<IndexMap<Self::Id, E>>;

    /// Retrieve a specific entity by ID.
    ///
    /// The storage value is automatically deserialized to the entity type.
    ///
    /// # Errors
    ///
    /// Returns an error if the entity doesn't exist or deserialization fails.
    async fn get(&self, id: &Self::Id) -> Result<E>;

    /// Retrieve one single entity from the set. If entities are ordered - return first entity.
    ///
    /// Useful for sampling data or checking if the dataset contains any entities.
    /// Returns `None` if the dataset is empty.
    async fn get_some(&self) -> Result<Option<(Self::Id, E)>>;
}

/// Write operations on typed entities with automatic serialization.
///
/// This trait provides convenient write operations that automatically handle
/// entity serialization to the storage format. All operations follow idempotent
/// patterns safe for retry in distributed systems.
///
/// # Serialization Behavior
///
/// Entities are automatically serialized to the storage's `Value` type before
/// persistence. The serialization format depends on your storage backend:
/// - JSON databases use `serde_json` serialization
/// - Binary stores may use CBOR or custom formats
/// - Document databases preserve nested structure
///
/// # Idempotency Guarantees
///
/// All write operations are designed to be safely retryable:
/// - `insert`: No-op if ID already exists
/// - `replace`: Always succeeds, overwrites existing data
/// - `patch`: Atomic update, fails if entity doesn't exist
///
/// # Example
///
/// ```rust,ignore
/// use vantage_dataset::dataset::WritableDataSet;
///
/// let user = User {
///     name: "Alice".to_string(),
///     email: "alice@example.com".to_string(),
///     age: 30,
/// };
///
/// // Idempotent insert
/// users.insert(&"user-123".to_string(), user.clone()).await?;
///
/// // Update specific fields
/// let mut updated_user = user;
/// updated_user.age = 31;
/// users.replace(&"user-123".to_string(), updated_user).await?;
/// ```
#[async_trait]
pub trait WritableDataSet<E>: DataSet<E>
where
    E: Entity<Self::Value>,
{
    /// Insert entity with a specific ID (often generated) (HTTP POST with ID)
    ///
    /// **Idempotent**: Succeeds if no entity exists with the given ID. If
    /// entity already exists, must return success without overwriting
    /// data, returning original data.
    ///
    /// **Returns**: Entity as it was stored.
    ///
    /// # Use Case
    /// Generate unique ID and store centity while avoiding duplicates.
    async fn insert(&self, id: &Self::Id, entity: &E) -> Result<E>;

    /// Replace the entire entity at the specified ID (HTTP PUT)
    ///
    /// **Idempotent**: Always succeeds, completely overwrites existing data
    /// if present. If possible, will remove/recreate entity; therefore if
    /// `entity` doesn't contain certain attributes which were present in the
    /// database, those will be removed. If entity does not exist, will
    /// create it.
    ///
    /// **Returns**: entity as it was stored.
    ///
    /// # Use Case
    /// Replace with a new version of a entity.
    async fn replace(&self, id: &Self::Id, entity: &E) -> Result<E>;

    /// Partially update an entity by merging with the provided data (HTTP PATCH)
    ///
    /// **Fails if entity doesn't exist**. The exact merge behavior depends on
    /// the storage implementation - typically merges object fields for JSON-like values.
    ///
    /// **Returns**: entity as it was stored (not only the partial change).
    ///
    /// # Use Case
    /// Update only the modified fields of a entity.
    async fn patch(&self, id: &Self::Id, partial: &E) -> Result<E>;

    /// Delete a entity by ID (HTTP DELETE)
    ///
    /// **Idempotent**: Always succeeds, even if the entity doesn't exist.
    /// This allows safe cleanup operations without checking existence first.
    async fn delete(&self, id: &Self::Id) -> Result<()>;

    /// Delete all entities in the set (HTTP DELETE without ID)
    ///
    /// **Idempotent**: All entities in the set will be deleted.
    /// Executing several times is OK.
    ///
    /// Execute on a subset of your entire database.
    async fn delete_all(&self) -> Result<()>;
}

/// Append-only operations with automatic ID generation.
///
/// This trait is designed for storage backends that naturally generate unique IDs
/// for new entities, such as message queues, event streams, or auto-incrementing
/// database tables.
///
/// # Idempotency Considerations
///
/// Unlike other dataset operations, `insert_return_id` is **not idempotent** because
/// each call generates a new ID. Use this pattern only when:
/// - Your system can handle duplicate entities (event sourcing)
/// - You have application-level deduplication
/// - The storage naturally handles uniqueness (like message queues)
///
/// For idempotent operations, prefer [`WritableDataSet::insert`] with predetermined IDs.
///
/// # Example
///
/// ```rust,ignore
/// use vantage_dataset::dataset::InsertableDataSet;
///
/// // Message queue scenario - each event gets unique ID
/// let event = UserLoginEvent {
///     user_id: "user-123".to_string(),
///     timestamp: Utc::now(),
///     ip_address: "192.168.1.1".to_string(),
/// };
///
/// let event_id = events.insert_return_id(event).await?;
/// println!("Generated event ID: {}", event_id);
/// ```
#[async_trait]
pub trait InsertableDataSet<E>: DataSet<E>
where
    E: Entity<Self::Value>,
{
    /// Insert an entity and return the generated ID.
    ///
    /// The storage backend generates a unique identifier for the new entity.
    /// The entity is automatically serialized to the storage format.
    ///
    /// # Warning
    ///
    /// This method is **not idempotent** - each call creates a new entity with
    /// a new ID, even if the entity data is identical.
    async fn insert_return_id(&self, entity: &E) -> Result<Self::Id>;
}

/// Change tracking for typed entities with automatic persistence.
///
/// This trait extends readable and writable datasets with a "entity" pattern that
/// tracks entity modifications and enables deferred persistence. entities act as
/// smart wrappers around entities that know how to save themselves back to storage.
///
/// # entity Pattern Benefits
///
/// - **Change tracking**: Only modified fields are serialized and persisted
/// - **Type safety**: Work with native Rust entities, not raw values
/// - **Optimistic locking**: Conflict detection in concurrent scenarios
/// - **Deferred persistence**: Batch multiple changes before saving
/// - **Interactive editing**: Perfect for UI scenarios with undo/redo
///
/// # Example
///
/// ```rust,ignore
/// use vantage_dataset::dataset::entityDataSet;
///
/// // Get entity for interactive editing
/// let mut user_entity = users.get_entity(&user_id).await?.unwrap();
///
/// // Modify through standard field access
/// user_entity.name = "Alice Smith".to_string();
/// user_entity.age = 31;
///
/// // Changes are automatically tracked and persisted
/// user_entity.save().await?;
///
/// // Or work with multiple entities
/// let mut entities = users.list_entities().await?;
/// for mut entity in entities {
///     entity.status = Status::Processed;
///     entity.save().await?; // Each saves independently
/// }
/// ```
#[async_trait]
pub trait ActiveEntitySet<E>: ReadableDataSet<E> + WritableDataSet<E>
where
    E: Entity<Self::Value>
        + vantage_types::IntoRecord<Self::Value>
        + vantage_types::TryFromRecord<Self::Value>
        + Send
        + Sync
        + Clone,
{
    /// Retrieve an entity wrapped for change tracking and deferred persistence.
    ///
    /// The returned [`entity`] can be modified in-place and will track all
    /// changes for efficient persistence when `save()` is called.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut user = table.get_entity(&"user123".to_string()).await?
    ///     .unwrap_or_else(|| table.new_entity("user123".to_string(), User {
    ///         id: Some("user123".to_string()),
    ///         name: "Default User".to_string(),
    ///         active: false,
    ///     }));
    ///
    /// user.active = true;
    /// user.save().await?;
    /// ```
    ///
    /// # Returns
    ///
    /// - `Ok(Some(entity))`: Entity wrapper with change tracking
    /// - `Ok(None)`: entity doesn't exist
    /// - `Err`: Storage or deserialization error
    async fn get_entity(&self, id: &Self::Id) -> Result<Option<ActiveEntity<'_, Self, E>>> {
        match self.get(id).await {
            Ok(data) => Ok(Some(ActiveEntity::new(id.clone(), data, self))),
            Err(_) => Ok(None),
        }
    }

    /// Retrieve all entities wrapped for change tracking.
    ///
    /// Each returned [`entity`] operates independently - modifications to one
    /// entity don't affect others, and each must be saved separately.
    ///
    /// # Performance Note
    ///
    /// This loads and deserializes all entities into memory. Consider pagination
    /// or streaming approaches for large datasets.
    async fn list_entities(&self) -> Result<Vec<ActiveEntity<'_, Self, E>>> {
        let items = self.list().await?;

        Ok(items
            .into_iter()
            .map(|(id, data)| ActiveEntity::new(id, data, self))
            .collect::<Vec<_>>())
    }

    /// Retrieve some entity wrapped for change tracking and deferred persistence.
    ///
    /// This is equivalent to get_some() but returns an ActiveEntity wrapper.
    ///
    /// # Returns
    ///
    /// - `Ok(Some(entity))`: Entity wrapper with change tracking
    /// - `Ok(None)`: no entities exist in the dataset
    /// - `Err`: Storage or deserialization error
    async fn get_some_entity(&self) -> Result<Option<ActiveEntity<'_, Self, E>>> {
        match self.get_some().await? {
            Some((id, data)) => Ok(Some(ActiveEntity::new(id, data, self))),
            None => Ok(None),
        }
    }

    /// Create a new entity with the provided data.
    ///
    /// This method creates a new entity and returns it wrapped as an ActiveEntity.
    /// The entity is not automatically saved - call `.save()` to persist it.
    ///
    /// # Parameters
    ///
    /// - `id`: The ID for the new entity
    /// - `entity`: The entity data
    ///
    /// # Returns
    ///
    /// - `ActiveEntity`: New entity wrapper ready for modification and saving
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut user = table.get_entity(&"user123".to_string()).await?
    ///     .unwrap_or_else(|| table.new_entity("user123".to_string(), User {
    ///         id: Some("user123".to_string()),
    ///         name: "Default User".to_string(),
    ///         active: false,
    ///     }));
    ///
    /// user.active = true;
    /// user.save().await?;
    /// ```
    ///
    /// # Note
    ///
    /// This method does not check if an entity with the given ID already exists.
    /// Use in combination with `get_entity()` for get-or-create patterns.
    fn new_entity(&self, id: Self::Id, entity: E) -> ActiveEntity<'_, Self, E> {
        ActiveEntity::new(id, entity, self)
    }
}
// Auto-implement for any type that has both readable and writable traits
impl<T, E> ActiveEntitySet<E> for T
where
    T: ReadableDataSet<E> + WritableDataSet<E>,
    E: Entity<T::Value>
        + vantage_types::IntoRecord<T::Value>
        + vantage_types::TryFromRecord<T::Value>
        + Send
        + Sync
        + Clone,
{
}

// // Auto-implement for any type that has both readable and writable traits
// #[async_trait]
// impl<T> entityValueSet for T
// where
//     T: ReadableValueSet + WritableValueSet,
//     Self::Value: Send + Sync + Clone,
// {
//     async fn get_value_entity(&self, id: &Self::Id) -> Result<entityValue<'_, Self>> {
//         let value = self.get_value(id).await?;
//         Ok(entityValue::new(id, value, self))
//     }

//     async fn list_value_entities(&self) -> Result<Vec<entityValue<'_, Self>>> {
//         let items = self.list_values().await?;

//         Ok(items
//             .into_iter()
//             .map(|(id, value)| entityValue::new(id, value, self))
//             .collect::<Vec<_>>())
//     }
// }
