mod deferred;
mod writes;

use std::cmp::Ordering;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_expressions::{AssociatedExpression, ExprDataSource, Expression, ExpressiveEnum};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};
use vantage_vista::FilterOp;

use super::MemoryDB;
use super::convert::{from_cbor_record, table_query};
use crate::eval::{MemoryCondition, matches_all};
use crate::types::AnyMemoryType;

#[async_trait]
impl TableSource for MemoryDB {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnyMemoryType;
    type Value = AnyMemoryType;
    type Id = String;
    type Condition = MemoryCondition;
    type Source = String;

    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type> {
        Column::new(name)
    }

    fn to_any_column<Type: ColumnType>(
        &self,
        column: Self::Column<Type>,
    ) -> Self::Column<Self::AnyType> {
        Column::from_column(column)
    }

    fn convert_any_column<Type: ColumnType>(
        &self,
        any_column: Self::Column<Self::AnyType>,
    ) -> Option<Self::Column<Type>> {
        Some(Column::from_column(any_column))
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<ExpressiveEnum<Self::Value>>,
    ) -> Expression<Self::Value> {
        Expression::new(template, parameters)
    }

    fn search_table_condition<E>(
        &self,
        table: &Table<Self, E>,
        search_value: &str,
    ) -> Self::Condition
    where
        E: Entity<Self::Value>,
    {
        MemoryCondition::Search {
            text: search_value.to_string(),
            columns: table
                .search_columns()
                .into_iter()
                .map(|column| column.name().to_string())
                .collect(),
        }
    }

    fn eq_condition(field: &str, value: &str) -> Result<Self::Condition> {
        Ok(MemoryCondition::cmp(
            field,
            FilterOp::Eq,
            CborValue::Text(value.to_string()),
        ))
    }

    fn condition_equality(&self, condition: &Self::Condition) -> Option<(String, Self::Value)> {
        match condition {
            MemoryCondition::Cmp {
                path,
                op: FilterOp::Eq,
                value,
            } => Some((path.clone(), AnyMemoryType::untyped(value.clone()))),
            _ => None,
        }
    }

    fn describe_condition(&self, condition: &Self::Condition) -> String {
        condition.describe()
    }

    fn condition_column(&self, condition: &Self::Condition) -> Option<String> {
        match condition {
            MemoryCondition::Cmp {
                op: FilterOp::Ne | FilterOp::NotInSet,
                ..
            } => None,
            MemoryCondition::Cmp { path, .. } | MemoryCondition::DeferredIn { path, .. } => {
                Some(path.clone())
            }
            _ => None,
        }
    }

    fn can_confine_writes(&self) -> bool {
        true
    }

    fn eq_value_condition(&self, field: &str, value: Self::Value) -> Result<Self::Condition> {
        Ok(MemoryCondition::cmp(
            field,
            FilterOp::Eq,
            value.into_value(),
        ))
    }

    fn in_values_condition(
        &self,
        field: &str,
        values: Vec<Self::Value>,
    ) -> Result<Self::Condition> {
        let set = values.into_iter().map(|v| v.into_value()).collect();
        Ok(MemoryCondition::cmp(
            field,
            FilterOp::InSet,
            ciborium::Value::Array(set),
        ))
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        Ok(self
            .rows(table, true)
            .await?
            .into_iter()
            .map(|(id, row)| (id, from_cbor_record(&row)))
            .collect())
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Option<Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        let Some(row) = self.store_table(table)?.get(id) else {
            return Ok(None);
        };
        let q = table_query(table).await?;
        Ok(matches_all(&q, &row)?.then(|| from_cbor_record(&row)))
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
    {
        let mut q = table_query(table).await?;
        q.limit = Some(1);
        let rows = self.store_table(table)?.query(&q)?;
        Ok(rows
            .into_iter()
            .next()
            .map(|(id, row)| (id, from_cbor_record(&row))))
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        let q = table_query(table).await?;
        Ok(self.store_table(table)?.count(&q)? as i64)
    }

    async fn get_table_sum<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        self.sum(table, column).await
    }

    async fn get_table_max<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        self.extreme(table, column, Ordering::Greater).await
    }

    async fn get_table_min<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        self.extreme(table, column, Ordering::Less).await
    }

    async fn insert_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        self.insert_in_set(table, id, record).await
    }

    async fn replace_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        self.replace_in_set(table, id, record).await
    }

    async fn patch_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        self.patch_in_set(table, id, partial).await
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        self.delete_in_set(table, id).await
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        self.delete_all_in_set(table).await
    }

    async fn insert_table_return_id_value<E>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
    {
        self.insert_new_in_set(table, record).await
    }

    fn related_in_condition<SourceE: Entity<Self::Value> + 'static>(
        &self,
        target_field: &str,
        source_table: &Table<Self, SourceE>,
        source_column: &str,
    ) -> Self::Condition
    where
        Self: Sized,
    {
        self.related_values(target_field, source_table, source_column)
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: ExprDataSource<Self::Value> + Sized,
    {
        let expr = self.column_values_expr(table, ColumnLike::name(column).to_string());
        AssociatedExpression::new(expr, self)
    }
}
