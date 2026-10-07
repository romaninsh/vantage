use std::cmp::Ordering;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_expressions::{
    AssociatedExpression, DeferredFn, ExprDataSource, Expression, ExpressiveEnum, expr_any,
};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};
use vantage_vista::FilterOp;

use super::MemoryDB;
use super::convert::{column_values, from_cbor_record, table_query, to_cbor_record, with_id_forms};
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
        _table: &Table<Self, E>,
        search_value: &str,
    ) -> Self::Condition
    where
        E: Entity<Self::Value>,
    {
        MemoryCondition::Search(search_value.to_string())
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
        let not_found = || {
            error!(
                "Row not found",
                table = table.table_name(),
                id = id.as_str()
            )
            .mark_not_found()
        };
        let store = self.store_table(table)?;
        let existing = store.get(id).ok_or_else(not_found)?;
        if !self.in_set(table, &existing).await? {
            return Err(not_found());
        }
        let id_column = self.id_column(table);
        let mut merged = (*existing).clone();
        for (k, v) in to_cbor_record(partial) {
            if k != id_column {
                merged.insert(k, v);
            }
        }
        if !self.in_set(table, &merged).await? {
            return Err(error!(
                "patch would move the row out of this set",
                table = table.table_name(),
                id = id.as_str()
            )
            .mark_conflict());
        }
        if !store.patch(id, &to_cbor_record(partial)) {
            return Err(not_found());
        }
        let row = store.get(id).ok_or_else(not_found)?;
        Ok(from_cbor_record(&row))
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let store = self.store_table(table)?;
        if let Some(row) = store.get(id)
            && self.in_set(table, &row).await?
        {
            store.delete(id);
        }
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let store = self.store_table(table)?;
        for (id, _) in self.rows(table, false).await? {
            store.delete(&id);
        }
        Ok(())
    }

    async fn insert_table_return_id_value<E>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
    {
        let row = to_cbor_record(record);
        if !self.in_set(table, &row).await? {
            // The id is assigned by the store after this check, so none is known yet.
            return Err(error!(
                "record does not belong to this set",
                table = table.table_name()
            )
            .mark_conflict());
        }
        self.store_table(table)?.insert(row)
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
        let db = self.clone();
        let source = source_table.clone();
        let col = source_column.to_string();
        let target = target_field.to_string();
        MemoryCondition::Deferred(DeferredFn::from_fn(move || {
            let (db, source, col, target) =
                (db.clone(), source.clone(), col.clone(), target.clone());
            async move {
                let rows = db.rows(&source, true).await?;
                let id_column = db.id_column(&source);
                let values = with_id_forms(column_values(&rows, &col, &id_column));
                let payload =
                    CborValue::Array(vec![CborValue::Text(target), CborValue::Array(values)]);
                Ok(AnyMemoryType::untyped(payload))
            }
        }))
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
        let db = self.clone();
        let table = table.clone();
        let col = ColumnLike::name(column).to_string();
        let inner = expr_any!("{}", {
            DeferredFn::new(move || {
                let (db, table, col) = (db.clone(), table.clone(), col.clone());
                Box::pin(async move {
                    let rows = db.rows(&table, true).await?;
                    let id_column = db.id_column(&table);
                    let values = column_values(&rows, &col, &id_column);
                    Ok(ExpressiveEnum::Scalar(AnyMemoryType::untyped(
                        CborValue::Array(values),
                    )))
                })
            })
        });
        let expr = expr_any!("{}", { self.defer(inner) });
        AssociatedExpression::new(expr, self)
    }
}
