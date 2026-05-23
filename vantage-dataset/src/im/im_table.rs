use uuid::Uuid;

use crate::{im::ImDataSource, traits::ValueSet};

/// Typed handle into one named table inside an [`ImDataSource`].
///
/// Generic over the entity type `E` and the wire value type `V` (defaults
/// to `serde_json::Value` for back-compat). The entity-typed dataset
/// impls (`ReadableDataSet`, `WritableDataSet`, `InsertableDataSet`) are
/// only available for `V = serde_json::Value` because they round-trip
/// through `serde_json` for `try_from_record`; the value-typed valueset
/// impls work for any `V`.
pub struct ImTable<E, V = serde_json::Value> {
    pub(super) data_source: ImDataSource<V>,
    pub(super) table_name: String,
    _phantom: std::marker::PhantomData<E>,
}

impl<E, V> ImTable<E, V> {
    pub fn new(data_source: &ImDataSource<V>, table_name: &str) -> Self
    where
        V: Clone,
    {
        Self {
            data_source: data_source.clone(),
            table_name: table_name.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn generate_id(&self) -> String {
        Uuid::new_v4().to_string()
    }
}

impl<E, V> ValueSet for ImTable<E, V>
where
    V: Clone + Send + Sync + 'static,
{
    type Id = String;
    type Value = V;
}
