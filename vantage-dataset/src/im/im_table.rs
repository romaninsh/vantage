use uuid::Uuid;

use crate::{im::ImDataSource, traits::ValueSet};

/// Table represents a typed table in the ImDataSource
pub struct ImTable<E> {
    pub(super) data_source: ImDataSource,
    pub(super) table_name: String,
    _phantom: std::marker::PhantomData<E>,
}

impl<E> ImTable<E> {
    pub fn new(data_source: &ImDataSource, table_name: &str) -> Self {
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

impl<E> ValueSet for ImTable<E> {
    type Id = String;
    type Value = serde_json::Value;
}
