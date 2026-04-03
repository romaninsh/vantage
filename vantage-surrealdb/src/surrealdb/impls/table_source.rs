use async_trait::async_trait;
use indexmap::IndexMap;

use vantage_dataset::traits::Result;
use vantage_expressions::Expression;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_table::column::core::{Column, ColumnType};

use vantage_table::table::Table;

use vantage_table::traits::table_like::TableLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::surrealdb::SurrealDB;
use crate::thing::Thing;
use crate::types::AnySurrealType;

#[async_trait]
impl TableSource for SurrealDB {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnySurrealType;
    type Value = AnySurrealType;
    type Id = Thing;

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

    fn search_table_expr(
        &self,
        _table: &impl TableLike,
        search_value: &str,
    ) -> Expression<Self::Value> {
        // TODO: iterate searchable columns once TableLike exposes them
        Expression::new(
            "SEARCH {}",
            vec![ExpressiveEnum::Scalar(AnySurrealType::new(
                search_value.to_string(),
            ))],
        )
    }

    async fn list_table_values<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        todo!("list_table_values: build SurrealSelect, execute, parse CBOR rows")
    }

    async fn get_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        todo!("get_table_value: SELECT * FROM ONLY table:id")
    }

    async fn get_table_some_value<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
    {
        todo!("get_table_some_value: SELECT * FROM table LIMIT 1")
    }

    async fn get_count<E>(&self, _table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        todo!("get_count: SELECT count() FROM table GROUP ALL")
    }

    async fn get_sum<E, Type: ColumnType>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
    {
        todo!("get_sum: SELECT math::sum(column) FROM table GROUP ALL")
    }

    async fn get_max<E, Type: ColumnType>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
    {
        todo!("get_max: SELECT math::max(column) FROM table GROUP ALL")
    }

    async fn get_min<E, Type: ColumnType>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
    {
        todo!("get_min: SELECT math::min(column) FROM table GROUP ALL")
    }

    async fn insert_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        todo!("insert_table_value: CREATE table:id SET ...")
    }

    async fn replace_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        todo!("replace_table_value: UPDATE table:id CONTENT ...")
    }

    async fn patch_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        todo!("patch_table_value: UPDATE table:id MERGE ...")
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        todo!("delete_table_value: DELETE table:id")
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        todo!("delete_table_all_values: DELETE table")
    }

    async fn insert_table_return_id_value<E>(
        &self,
        _table: &Table<Self, E>,
        _record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
    {
        todo!("insert_table_return_id_value: CREATE table SET ... RETURN id")
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: ExprDataSource<Self::Value> + Sized,
    {
        todo!("column_table_values_expr: subquery for column values")
    }
}
