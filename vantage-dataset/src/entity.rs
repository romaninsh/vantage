/// Entity trait for types that can be used with datasets
///
/// Entities must be serializable, deserializable, and support basic operations
/// required for dataset manipulation across different data sources.
pub trait Entity:
    serde::Serialize + serde::de::DeserializeOwned + Default + Clone + Send + Sync + Sized + 'static
{
}

/// Auto-implement Entity for all types that satisfy the required bounds
impl<T> Entity for T where
    T: serde::Serialize
        + serde::de::DeserializeOwned
        + Default
        + Clone
        + Send
        + Sync
        + Sized
        + 'static
{
}
