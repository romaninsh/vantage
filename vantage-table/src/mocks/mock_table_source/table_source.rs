use async_trait::async_trait;
use indexmap::IndexMap;
use rust_decimal::Decimal;
use serde_json::Value;
use vantage_dataset::{ReadableValueSet, im::ImTable, traits::Result};
use vantage_expressions::{
    Expression, traits::associated_expressions::AssociatedExpression,
    traits::expressive::ExpressiveEnum,
};
use vantage_types::{Entity, Record};

use super::MockTableSource;
use crate::column::core::ColumnType;
use crate::mocks::mock_column::MockColumn;
use crate::mocks::mock_type_system::AnyMockType;
use crate::{
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

#[async_trait]
impl TableSource for MockTableSource {
    type Column<Type>
        = MockColumn<Type>
    where
        Type: ColumnType;
    type AnyType = AnyMockType;
    type Value = Value;
    type Id = String;
    type Condition = vantage_expressions::Expression<Self::Value>;
    type Source = String;

    fn condition_equality(&self, condition: &Expression<Value>) -> Option<(String, Value)> {
        crate::conditions::literal_equality(condition)
    }

    fn can_confine_writes(&self) -> bool {
        self.confines
    }

    fn eq_value_condition(&self, field: &str, value: Self::Value) -> Result<Self::Condition> {
        Ok(Expression::new(
            "{} = {}",
            vec![
                ExpressiveEnum::Nested(Expression::new(field.to_string(), vec![])),
                ExpressiveEnum::Nested(Expression::new("{}", vec![ExpressiveEnum::Scalar(value)])),
            ],
        ))
    }

    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type> {
        use std::any::TypeId;
        let type_id = TypeId::of::<Type>();

        if type_id != TypeId::of::<String>()
            && type_id != TypeId::of::<i64>()
            && type_id != TypeId::of::<f64>()
            && type_id != TypeId::of::<Decimal>()
            && type_id != TypeId::of::<bool>()
            && type_id != TypeId::of::<Option<String>>()
            && type_id != TypeId::of::<Option<i64>>()
            && type_id != TypeId::of::<Option<f64>>()
            && type_id != TypeId::of::<Option<Decimal>>()
            && type_id != TypeId::of::<Option<bool>>()
            // The dynamic carrier itself — what `to_any_column` stores, and
            // what type-agnostic columns (lazy expressions) register as.
            && type_id != TypeId::of::<AnyMockType>()
        {
            panic!(
                "Type {:?} is not compatible with mock_type_system. Only String, i64, f64, Decimal, bool and their Optionals are supported.",
                std::any::type_name::<Type>()
            );
        }

        MockColumn::new(name)
    }

    fn to_any_column<Type: ColumnType>(
        &self,
        column: Self::Column<Type>,
    ) -> Self::Column<Self::AnyType> {
        column.into_type()
    }

    fn convert_any_column<Type: ColumnType>(
        &self,
        any_column: Self::Column<Self::AnyType>,
    ) -> Option<Self::Column<Type>> {
        Some(any_column.into_type())
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
    ) -> Expression<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        Self::search_condition(table, search_value)
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        let mut rows = im_table.list_values().await?;
        let mut failure = None;
        rows.retain(|_, row| match Self::row_in_set(table, row) {
            Ok(keep) => keep,
            Err(e) => {
                failure.get_or_insert(e);
                false
            }
        });
        match failure {
            Some(e) => Err(e),
            None => Ok(rows),
        }
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Option<Record<Self::Value>>>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        match im_table.get_value(id).await? {
            Some(row) if Self::row_in_set(table, &row)? => Ok(Some(row)),
            _ => Ok(None),
        }
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.get_some_value().await
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity,
        Self: Sized,
    {
        if table.conditions().next().is_none() {
            return Ok(self.im_data_source.table_len(table.table_name()) as i64);
        }
        Ok(self.list_table_values(table).await?.len() as i64)
    }

    async fn get_table_sum<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(vantage_core::error!(
            "Sum not implemented for MockTableSource"
        ))
    }

    async fn get_table_max<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(vantage_core::error!(
            "Max not implemented for MockTableSource"
        ))
    }

    async fn get_table_min<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(vantage_core::error!(
            "Min not implemented for MockTableSource"
        ))
    }

    async fn insert_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity,
        Self: Sized,
    {
        self.insert_row(table, id, record).await
    }

    async fn replace_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity,
        Self: Sized,
    {
        self.replace_row(table, id, record).await
    }

    async fn patch_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity,
        Self: Sized,
    {
        self.patch_row(table, id, partial).await
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        self.delete_row(table, id).await
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        self.delete_rows(table).await
    }

    async fn insert_table_return_id_value<E>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity,
        Self: Sized,
    {
        self.insert_row_return_id(table, record).await
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
        let tgt_col: Expression<Value> = Expression::new(target_field, vec![]);
        Expression::new(
            "{} IN ({})",
            vec![
                ExpressiveEnum::Nested(tgt_col),
                ExpressiveEnum::Nested(self.column_subquery(source_table, source_column)),
            ],
        )
    }

    fn supports_traversal(&self) -> bool {
        true
    }

    fn related_correlated_condition(
        &self,
        target_table: &str,
        target_field: &str,
        source_table: &str,
        source_column: &str,
    ) -> Self::Condition {
        // Correlated equality rendered as a literal string — the
        // table-qualified form the SQL backends emit, enough for preview tests.
        Expression::new(
            format!("{target_table}.{target_field} = {source_table}.{source_column}"),
            vec![],
        )
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: Sized,
    {
        AssociatedExpression::new(self.column_subquery(table, column.name()), self)
    }
}
