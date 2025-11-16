use std::{marker::PhantomData, sync::Arc};

use indexmap::IndexMap;
use vantage_core::Entity;
use vantage_expressions::Expression;

use crate::{
    pagination::Pagination, /* references::RelatedTable, */ sorting::SortDirection,
    traits::table_source::TableSource,
};

#[derive(Clone)]
pub struct Table<T, E>
where
    T: TableSource,
    E: Entity,
{
    pub(super) data_source: T,
    pub(super) _phantom: PhantomData<E>,
    pub(super) table_name: String,
    pub(super) columns: IndexMap<String, T::Column>,
    pub(super) conditions: IndexMap<i64, Expression<T::Value>>,
    pub(super) next_condition_id: i64,
    pub(super) order_by: IndexMap<i64, (Expression<T::Value>, SortDirection)>,
    pub(super) next_order_id: i64,
    // pub(super) refs: Option<IndexMap<String, Arc<dyn RelatedTable>>>,
    pub(super) pagination: Option<Pagination>,
    pub(super) title_field: Option<String>,
    pub(super) id_field: Option<String>,
}

impl<T: TableSource, E: Entity> Table<T, E> {
    /// Create a new Table with the given table name and data source
    pub fn new(table_name: impl Into<String>, data_source: T) -> Self {
        Self {
            data_source,
            _phantom: PhantomData,
            table_name: table_name.into(),
            columns: IndexMap::new(),
            conditions: IndexMap::new(),
            next_condition_id: 1,
            order_by: IndexMap::new(),
            next_order_id: 1,
            // refs: None,
            pagination: None,
            title_field: None,
            id_field: None,
        }
    }

    /// Convert this table to use a different entity type
    pub fn into_entity<E2: Entity>(self) -> Table<T, E2> {
        Table {
            data_source: self.data_source,
            _phantom: PhantomData,
            table_name: self.table_name,
            columns: self.columns,
            conditions: self.conditions,
            next_condition_id: self.next_condition_id,
            order_by: self.order_by,
            next_order_id: self.next_order_id,
            // refs: self.refs,
            pagination: self.pagination,
            title_field: self.title_field,
            id_field: self.id_field,
        }
    }

    /// Use a callback with a builder pattern for configuration
    pub fn with<F>(mut self, func: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        func(&mut self);
        self
    }

    /// Get the table name
    pub fn table_name(&self) -> &str {
        &self.table_name
    }

    /// Get the underlying data source
    pub fn data_source(&self) -> &T {
        &self.data_source
    }

    /// Get mutable access to conditions (pub(crate) for TableLike impl)
    pub(crate) fn conditions_mut(&mut self) -> &mut IndexMap<i64, Expression<T::Value>> {
        &mut self.conditions
    }

    /// Get mutable access to next_condition_id (pub(crate) for TableLike impl)
    pub(crate) fn next_condition_id_mut(&mut self) -> &mut i64 {
        &mut self.next_condition_id
    }

    /// Get the title field column if set
    pub fn title_field(&self) -> Option<&T::Column> {
        self.title_field
            .as_ref()
            .and_then(|name| self.columns.get(name))
    }

    /// Get the id field column if set
    pub fn id_field(&self) -> Option<&T::Column> {
        self.id_field
            .as_ref()
            .and_then(|name| self.columns.get(name))
    }
}

impl<T: TableSource, E: Entity> std::fmt::Debug for Table<T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Table")
            .field("table_name", &self.table_name)
            .field("columns", &self.columns.keys().collect::<Vec<_>>())
            .field("conditions_count", &self.conditions.len())
            // .field(
            //     "refs_count",
            //     &self.refs.as_ref().map(|r| r.len()).unwrap_or(0),
            // )
            .finish()
    }
}
