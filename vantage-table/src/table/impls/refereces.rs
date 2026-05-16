//! Table relationship methods for defining and traversing references.

use indexmap::IndexMap;
use std::sync::Arc;

use vantage_core::{Result, error};
use vantage_expressions::Expression;
use vantage_types::{EmptyEntity, Entity, Record};

use crate::{
    any::AnyTable,
    references::{HasMany, HasOne, Reference},
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
        T::Value: Into<ciborium::Value> + From<ciborium::Value>,
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
        T::Value: Into<ciborium::Value> + From<ciborium::Value>,
        T::Id: std::fmt::Display + From<String>,
    {
        let reference = HasMany::<T, E, E2>::new(foreign_key, build_target);
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

    /// Narrow the table to a single row by id.
    ///
    /// Pairs with `get_some_value` for the "I only know an id" workflow.
    /// The actual condition construction goes through
    /// `TableSource::eq_value_condition`, so backends that don't yet
    /// implement that path return an error here.
    pub fn with_id(mut self, id: impl Into<T::Value>) -> Result<Self> {
        let id_name = self
            .id_field()
            .ok_or_else(|| error!("id field not set on table"))?
            .name()
            .to_string();
        let condition = self.data_source().eq_value_condition(&id_name, id.into())?;
        self.add_condition(condition);
        Ok(self)
    }

    /// Traverse a same-persistence reference using a known source row as the
    /// join origin.
    ///
    /// Reads the join field value out of `row`, builds the target table via
    /// the reference's stored factory, and applies one eq-condition that
    /// selects the related rows. No subquery, no deferred fetch — `row`
    /// already carries the value.
    ///
    /// `HasOne` reads from its stored foreign-key column; `HasMany` reads
    /// from the source's id field (looked up here and forwarded into the
    /// reference). The returned table preserves columns, refs, and
    /// expressions from the reference's factory; only the entity type
    /// changes if `E2` differs from the factory's output.
    pub fn get_ref_from_row<E2: Entity<T::Value> + 'static>(
        &self,
        relation: &str,
        row: &Record<T::Value>,
    ) -> Result<Table<T, E2>> {
        let (reference, _) = self.lookup_ref(relation)?;
        let source_id = self
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let target_dyn = reference.resolve_from_row(
            self.data_source() as &dyn std::any::Any,
            &source_id,
            row as &dyn std::any::Any,
        )?;

        let target_empty: Table<T, EmptyEntity> = *target_dyn
            .downcast::<Table<T, EmptyEntity>>()
            .map_err(|_| error!("Failed to downcast target table to Table<T, EmptyEntity>"))?;

        Ok(target_empty.into_entity::<E2>())
    }

    /// Get a same-backend related table with automatic downcasting.
    ///
    /// Legacy AnyTable-flavoured path; slated for deletion in Stage 9 alongside
    /// `AnyTable`. New code should prefer [`get_ref_from_row`] (typed) or
    /// `Vista::get_ref` (erased).
    pub fn get_ref_as<E2: Entity<T::Value> + 'static>(
        &self,
        relation: &str,
    ) -> Result<Table<T, E2>> {
        let (reference, relation_str) = self.lookup_ref(relation)?;

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

    /// Get a related table as AnyTable.
    ///
    /// Legacy AnyTable-flavoured path; slated for deletion in Stage 9.
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

    /// Look up cardinality for a registered relation.
    pub fn ref_cardinality(&self, relation: &str) -> Result<crate::references::Cardinality> {
        let (reference, _) = self.lookup_ref(relation)?;
        Ok(reference.cardinality())
    }

    /// List all registered relations with their cardinality.
    pub fn ref_kinds(&self) -> Vec<(String, crate::references::Cardinality)> {
        self.refs
            .as_ref()
            .map(|refs| {
                refs.iter()
                    .map(|(name, r)| (name.clone(), r.cardinality()))
                    .collect()
            })
            .unwrap_or_default()
    }
}
