//! Table relationship methods for defining and traversing one-to-one and one-to-many relationships
//!
//! This module provides methods for adding and accessing table relationships in the 0.3 architecture.

use indexmap::IndexMap;
use std::sync::Arc;

use vantage_core::{Result, error};

use crate::{
    ColumnLike, Entity, Table, TableSource,
    any::AnyTable,
    references::{ReferenceMany, ReferenceOne, RelatedTable},
};

impl<T: TableSource + 'static, E: Entity> Table<T, E> {
    /// Define a one-to-one relationship
    ///
    /// # Arguments
    /// * `relation` - Name for this relationship (e.g., "bakery")
    /// * `foreign_key` - Column name on this table (e.g., "bakery_id")
    /// * `get_table` - Closure that returns the related table
    ///
    /// # Example
    /// ```rust,ignore
    /// let clients = Table::new("client", db)
    ///     .with_one("bakery", "bakery_id", || Bakery::table(db.clone()));
    /// ```
    pub fn with_one<E2: Entity + 'static>(
        mut self,
        relation: &str,
        foreign_key: &str,
        get_table: impl Fn() -> Table<T, E2> + Send + Sync + 'static,
    ) -> Self
    where
        T: 'static + vantage_expressions::SelectSource<T::Expr>,
        T::Select<E>: Into<T::Expr>,
        T::Select<E2>: Into<T::Expr>,
        T::Column: ColumnLike,
    {
        let reference = ReferenceOne::<T, E, E2>::new(foreign_key, get_table);
        self.add_ref(relation, Box::new(reference));
        self
    }

    /// Define a one-to-many relationship
    ///
    /// # Arguments
    /// * `relation` - Name for this relationship (e.g., "orders")
    /// * `foreign_key` - Column name on target table (e.g., "client_id")
    /// * `get_table` - Closure that returns the related table
    ///
    /// # Example
    /// ```rust,ignore
    /// let clients = Table::new("client", db)
    ///     .with_many("orders", "client_id", || Order::table(db.clone()));
    /// ```
    pub fn with_many<E2: Entity + 'static>(
        mut self,
        relation: &str,
        foreign_key: &str,
        get_table: impl Fn() -> Table<T, E2> + Send + Sync + 'static,
    ) -> Self
    where
        T: 'static + vantage_expressions::SelectSource<T::Expr>,
        T::Select<E>: Into<T::Expr>,
        T::Select<E2>: Into<T::Expr>,
        T::Column: ColumnLike,
    {
        let reference = ReferenceMany::<T, E, E2>::new(foreign_key, get_table);
        self.add_ref(relation, Box::new(reference));
        self
    }

    /// Add a reference manually (internal use)
    pub(crate) fn add_ref(&mut self, relation: &str, reference: Box<dyn RelatedTable>) {
        if self.refs.is_none() {
            self.refs = Some(IndexMap::new());
        }
        self.refs
            .as_mut()
            .unwrap()
            .insert(relation.to_string(), Arc::from(reference));
    }

    /// Get list of available reference names
    pub fn references(&self) -> Vec<String> {
        self.refs
            .as_ref()
            .map(|refs| refs.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Get a related table as AnyTable
    ///
    /// Returns the related table with appropriate conditions applied.
    ///
    /// # Example
    /// ```rust,ignore
    /// let bakery_table = client.get_ref("bakery")?;
    /// ```
    pub fn get_ref(&self, relation: &str) -> Result<AnyTable> {
        let table_name = self.table_name.clone();
        let refs = self.refs.as_ref().ok_or_else(|| {
            error!(
                "No references defined on table",
                table = table_name.as_str()
            )
        })?;

        let relation_str = relation.to_string();
        let reference = refs.get(relation).ok_or_else(|| {
            error!(
                "Reference not found on table",
                relation = relation_str.as_str(),
                table = table_name.as_str()
            )
        })?;

        // Pass concrete table as &dyn Any instead of AnyTable
        reference.get_related_table(self as &dyn std::any::Any)
    }

    /// Get a related table with automatic downcasting
    ///
    /// This is a convenience method that combines `get_ref` and `downcast`.
    /// Returns error if reference not found or downcast fails.
    ///
    /// # Example
    /// ```rust,ignore
    /// let bakery: Table<SurrealDB, Bakery> = client.get_ref_as("bakery")?;
    /// ```
    pub fn get_ref_as<T2: TableSource + 'static, E2: Entity + 'static>(
        &self,
        relation: &str,
    ) -> Result<Table<T2, E2>> {
        let relation_str = relation.to_string();
        let table_name = self.table_name.clone();
        self.get_ref(relation)?.downcast::<T2, E2>().map_err(|e| {
            error!(
                "Failed to downcast reference from table",
                relation = relation_str.as_str(),
                table = table_name.as_str(),
                error = e.to_string().as_str()
            )
        })
    }

    /// Get a linked table (for subqueries/JOINs)
    ///
    /// Similar to `get_ref` but uses direct column equality instead of IN subquery.
    pub fn get_subquery(&self, relation: &str) -> Result<AnyTable> {
        let table_name = self.table_name.clone();
        let refs = self.refs.as_ref().ok_or_else(|| {
            error!(
                "No references defined on table",
                table = table_name.as_str()
            )
        })?;

        let relation_str = relation.to_string();
        let reference = refs.get(relation).ok_or_else(|| {
            error!(
                "Reference not found on table",
                relation = relation_str.as_str(),
                table = table_name.as_str()
            )
        })?;

        // Pass concrete table as &dyn Any instead of AnyTable
        reference.get_linked_table(self as &dyn std::any::Any)
    }

    /// Get a linked table with automatic downcasting
    pub fn get_subquery_as<T2: TableSource + 'static, E2: Entity + 'static>(
        &self,
        relation: &str,
    ) -> Result<Table<T2, E2>> {
        let relation_str = relation.to_string();
        let table_name = self.table_name.clone();
        self.get_subquery(relation)?
            .downcast::<T2, E2>()
            .map_err(|e| {
                error!(
                    "Failed to downcast subquery from table",
                    relation = relation_str.as_str(),
                    table = table_name.as_str(),
                    error = e.to_string().as_str()
                )
            })
    }
}

// Tests are in references.rs since they require SelectSource trait
