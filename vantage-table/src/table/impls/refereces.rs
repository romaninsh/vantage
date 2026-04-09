//! Table relationship methods for defining and traversing one-to-one and one-to-many relationships
//!
//! This module provides methods for adding and accessing table relationships in the 0.3 architecture.

use indexmap::IndexMap;
use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_expressions::Expression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_types::Entity;

use crate::{
    references::{ReferenceMany, ReferenceOne, RelatedTable, RelatedTableExt},
    table::Table,
    traits::table_source::TableSource,
};

impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> Table<T, E> {
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
    pub fn with_one<E2: Entity<T::Value> + 'static>(
        mut self,
        relation: &str,
        foreign_key: &str,
        get_table: impl Fn() -> Table<T, E2> + Send + Sync + 'static,
    ) -> Self
    where
        T: ExprDataSource<T::Value>,
        T::Value: Clone + Send + Sync + 'static,
        T::Column<T::AnyType>: crate::operation::Operation<T::Value>,
        T::Condition: From<Expression<T::Value>>,
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
    pub fn with_many<E2: Entity<T::Value> + 'static>(
        mut self,
        relation: &str,
        foreign_key: &str,
        get_table: impl Fn() -> Table<T, E2> + Send + Sync + 'static,
    ) -> Self
    where
        T: ExprDataSource<T::Value>,
        T::Value: Clone + Send + Sync + 'static,
        T::Column<T::AnyType>: crate::operation::Operation<T::Value>,
        T::Condition: From<Expression<T::Value>>,
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

    /// Get a related table as `Box<dyn Any>`
    pub fn get_ref(&self, relation: &str) -> Result<Box<dyn std::any::Any>> {
        let table_name = self.table_name().to_string();
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

        reference.get_related_table(self as &dyn std::any::Any)
    }

    /// Get a related table with automatic downcasting
    pub fn get_ref_as<T2: TableSource + 'static, E2: Entity<T2::Value> + 'static>(
        &self,
        relation: &str,
    ) -> Result<Table<T2, E2>> {
        let table_name = self.table_name().to_string();
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

        reference
            .as_ref()
            .as_table::<T2, E2>(self as &dyn std::any::Any)
    }

    /// Get a linked table (for subqueries/JOINs)
    pub fn get_subquery(&self, relation: &str) -> Result<Box<dyn std::any::Any>> {
        let table_name = self.table_name().to_string();
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

        reference.get_linked_table(self as &dyn std::any::Any)
    }

    /// Get a linked table with automatic downcasting
    pub fn get_subquery_as<T2: TableSource + 'static, E2: Entity<T2::Value> + 'static>(
        &self,
        relation: &str,
    ) -> Result<Table<T2, E2>> {
        let table_name = self.table_name().to_string();
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

        let boxed_table = reference.get_linked_table(self as &dyn std::any::Any)?;
        boxed_table
            .downcast::<Table<T2, E2>>()
            .map(|boxed| *boxed)
            .map_err(|_| error!("Failed to downcast linked table"))
    }
}
