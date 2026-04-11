//! Table relationship methods for defining and traversing references.

use indexmap::IndexMap;
use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_expressions::Expression;
use vantage_types::Entity;

use crate::{
    any::AnyTable,
    references::{HasForeign, HasMany, HasOne, Reference},
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

impl<T: TableSource + 'static, E: Entity<T::Value> + 'static> Table<T, E> {
    /// Define a one-to-one relationship.
    ///
    /// ```rust,ignore
    /// .with_one("bakery", "bakery_id", Bakery::postgres_table)
    /// ```
    pub fn with_one<E2: Entity<T::Value> + 'static>(
        mut self,
        relation: &str,
        foreign_key: &str,
        build_target: impl Fn(T) -> Table<T, E2> + Send + Sync + 'static,
    ) -> Self
    where
        T::Value: Into<serde_json::Value> + From<serde_json::Value>,
        T::Id: std::fmt::Display + From<String>,
    {
        let reference = HasOne::<T, E, E2>::new(foreign_key, build_target);
        self.add_ref(relation, Box::new(reference));
        self
    }

    /// Define a one-to-many relationship.
    ///
    /// ```rust,ignore
    /// .with_many("orders", "client_id", Order::postgres_table)
    /// ```
    pub fn with_many<E2: Entity<T::Value> + 'static>(
        mut self,
        relation: &str,
        foreign_key: &str,
        build_target: impl Fn(T) -> Table<T, E2> + Send + Sync + 'static,
    ) -> Self
    where
        T::Value: Into<serde_json::Value> + From<serde_json::Value>,
        T::Id: std::fmt::Display + From<String>,
    {
        let reference = HasMany::<T, E, E2>::new(foreign_key, build_target);
        self.add_ref(relation, Box::new(reference));
        self
    }

    /// Define a cross-persistence reference.
    ///
    /// The closure receives this table and returns an `AnyTable` from any backend
    /// with deferred conditions attached.
    ///
    /// ```rust,ignore
    /// .with_foreign("mongo_orders", "Table<MongoDB, Order>", |clients| {
    ///     let mut orders = Order::mongo_table(mongo_db.clone());
    ///     // attach deferred condition ...
    ///     Ok(AnyTable::from_table(orders))
    /// })
    /// ```
    pub fn with_foreign(
        mut self,
        relation: &str,
        target_type: &'static str,
        resolve: impl Fn(&Table<T, E>) -> Result<AnyTable> + Send + Sync + 'static,
    ) -> Self {
        let reference = HasForeign::<T, E>::new(target_type, resolve);
        self.add_ref(relation, Box::new(reference));
        self
    }

    pub(crate) fn add_ref(&mut self, relation: &str, reference: Box<dyn Reference>) {
        if self.refs.is_none() {
            self.refs = Some(IndexMap::new());
        }
        self.refs
            .as_mut()
            .unwrap()
            .insert(relation.to_string(), Arc::from(reference));
    }

    pub fn references(&self) -> Vec<String> {
        self.refs
            .as_ref()
            .map(|refs| refs.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Check if a reference is cross-persistence (foreign).
    pub fn is_foreign_ref(&self, relation: &str) -> Result<bool> {
        let (reference, _) = self.lookup_ref(relation)?;
        Ok(reference.is_foreign())
    }

    /// Get a same-backend related table with automatic downcasting.
    ///
    /// For foreign references, use `get_ref()` instead.
    pub fn get_ref_as<E2: Entity<T::Value> + 'static>(
        &self,
        relation: &str,
    ) -> Result<Table<T, E2>> {
        let (reference, relation_str) = self.lookup_ref(relation)?;

        if reference.is_foreign() {
            return Err(error!(
                "Cannot use get_ref_as for foreign references, use get_ref instead",
                relation = relation_str.as_str()
            ));
        }

        // 1. Build target
        let source_id = self
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let mut target: Table<T, E2> = *reference
            .build_target(self.data_source() as &dyn std::any::Any)
            .downcast::<Table<T, E2>>()
            .map_err(|_| {
                error!(
                    "Failed to downcast related table",
                    relation = relation_str.as_str()
                )
            })?;

        // 2. Get columns
        let target_id = target
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let (src_col, tgt_col) = reference.columns(&source_id, &target_id);

        // 3. Build and apply condition
        let condition = self
            .data_source()
            .related_in_condition(&tgt_col, self, &src_col);
        target.add_condition(condition);

        Ok(target)
    }

    /// Get a related table as AnyTable — works for both same-backend and foreign refs.
    pub fn get_ref(&self, relation: &str) -> Result<AnyTable> {
        let (reference, _) = self.lookup_ref(relation)?;
        reference.resolve_as_any(self as &dyn std::any::Any)
    }

    /// Get a correlated related table for use inside SELECT expressions.
    ///
    /// Unlike `get_ref_as` (which uses `IN (subquery)`), this produces a
    /// correlated condition like `order.client_id = client.id`, suitable
    /// for embedding as a subquery in a SELECT clause.
    pub fn get_subquery_as<E2: Entity<T::Value> + 'static>(
        &self,
        relation: &str,
    ) -> Result<Table<T, E2>> {
        let (reference, relation_str) = self.lookup_ref(relation)?;

        if reference.is_foreign() {
            return Err(error!(
                "Cannot use get_subquery_as for foreign references",
                relation = relation_str.as_str()
            ));
        }

        // 1. Build target
        let source_id = self
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let mut target: Table<T, E2> = *reference
            .build_target(self.data_source() as &dyn std::any::Any)
            .downcast::<Table<T, E2>>()
            .map_err(|_| {
                error!(
                    "Failed to downcast related table",
                    relation = relation_str.as_str()
                )
            })?;

        // 2. Get columns
        let target_id = target
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let (src_col, tgt_col) = reference.columns(&source_id, &target_id);

        // 3. Build correlated condition: target_table.tgt_col = source_table.src_col
        let condition = self.data_source().related_correlated_condition(
            target.table_name(),
            &tgt_col,
            self.table_name(),
            &src_col,
        );
        target.add_condition(condition);

        Ok(target)
    }

    /// Add a computed expression field using builder pattern.
    ///
    /// The closure receives `&Table<T, E>` and returns an `Expression<T::Value>`.
    /// It is evaluated lazily when `select()` builds the query.
    ///
    /// ```rust,ignore
    /// .with_expression("order_count", |t| {
    ///     t.get_subquery_as::<Order>("orders").unwrap().get_count_query()
    /// })
    /// ```
    pub fn with_expression(
        mut self,
        name: &str,
        expr_fn: impl Fn(&Table<T, E>) -> Expression<T::Value> + Send + Sync + 'static,
    ) -> Self {
        self.expressions.insert(name.to_string(), Arc::new(expr_fn));
        self
    }

    fn lookup_ref(&self, relation: &str) -> Result<(&dyn Reference, String)> {
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

        Ok((reference.as_ref(), relation_str))
    }
}
