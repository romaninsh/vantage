use indexmap::IndexMap;
use uuid::Uuid;
use vantage_types::Record;

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

    /// Synchronously replace this table's rows with `records` (ordered).
    /// Lets callers seed an in-memory table from a known collection without
    /// going through the async insert path — used to materialize a contained
    /// relation's records from a parent row's embedded column.
    pub fn seed(&self, records: IndexMap<String, Record<V>>)
    where
        V: Clone,
    {
        self.data_source
            .with_table_mut(&self.table_name, |table| *table = records);
    }
}

impl<E, V> ValueSet for ImTable<E, V>
where
    V: Clone + Send + Sync + 'static,
{
    type Id = String;
    type Value = V;
}
