use async_trait::async_trait;
use indexmap::IndexMap;

use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};
use vantage_table::column::core::{Column, ColumnType};

use vantage_table::table::Table;

use vantage_table::traits::table_like::TableLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::surrealdb::SurrealDB;
use crate::thing::Thing;
use crate::types::{AnySurrealType, SurrealType};

/// Parse a CBOR map into a Record and optionally extract the ID field as a Thing.
fn parse_cbor_row(
    map: Vec<(ciborium::Value, ciborium::Value)>,
    id_field_name: &str,
) -> (Option<Thing>, Record<AnySurrealType>) {
    let mut fields = IndexMap::new();
    let mut thing: Option<Thing> = None;

    for (k, v) in map {
        let key = match k {
            ciborium::Value::Text(s) => s,
            _ => continue,
        };
        if key == id_field_name {
            thing = Thing::from_cbor(v.clone());
        }
        if let Some(val) = AnySurrealType::from_cbor(&v) {
            fields.insert(key, val);
        }
    }

    (thing, Record::from_indexmap(fields))
}

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
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        let id_field_name = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let select = super::build_select::build_select(table);
        let result = self.execute(&select.expr()).await?;

        let arr = result
            .into_value()
            .into_array()
            .map_err(|_| error!("list_table_values: expected array result"))?;

        let mut records = IndexMap::new();
        for item in arr {
            let map = match item {
                ciborium::Value::Map(m) => m,
                _ => continue,
            };

            let (thing, record) = parse_cbor_row(map, &id_field_name);
            let id = thing.ok_or_else(|| {
                error!(
                    "list_table_values: row missing id field",
                    id_field = &id_field_name
                )
            })?;
            records.insert(id, record);
        }

        Ok(records)
    }

    async fn get_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        let query = crate::surreal_expr!("SELECT * FROM ONLY {}", (id.clone()));
        let result = self.execute(&query).await?;

        let map = result.into_value().into_map().map_err(|_| {
            error!(
                "get_table_value: expected map result",
                id = format!("{:?}", id)
            )
        })?;

        let (_thing, record) = parse_cbor_row(map, "id");
        Ok(record)
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
    {
        let mut select = super::build_select::build_select(table);
        select.limit = Some(1);
        let result = self.execute(&select.expr()).await?;

        let id_field_name = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let arr = result
            .into_value()
            .into_array()
            .map_err(|_| error!("get_table_some_value: expected array result"))?;

        let item = match arr.into_iter().next() {
            Some(item) => item,
            None => return Ok(None),
        };

        let map = match item {
            ciborium::Value::Map(m) => m,
            _ => return Ok(None),
        };

        let (thing, record) = parse_cbor_row(map, &id_field_name);
        match thing {
            Some(id) => Ok(Some((id, record))),
            None => Ok(None),
        }
    }

    async fn get_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        let mut select = super::build_select::build_select(table);
        select.order_by.clear(); // ordering is unnecessary for count
        let count_query = select.as_count();
        let result = self.execute(&count_query.expr()).await?;
        result.try_get::<i64>().ok_or_else(|| {
            vantage_core::error!("get_count: expected i64", result = format!("{}", result))
        })
    }

    async fn get_sum<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let mut select = super::build_select::build_select(table);
        select.order_by.clear();
        let sum_query = select.as_sum(column.clone());
        self.execute(&sum_query.expr()).await
    }

    async fn get_max<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let mut select = super::build_select::build_select(table);
        select.order_by.clear();
        let max_query = select.as_max(column.clone());
        self.execute(&max_query.expr()).await
    }

    async fn get_min<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let mut select = super::build_select::build_select(table);
        select.order_by.clear();
        let min_query = select.as_min(column.clone());
        self.execute(&min_query.expr()).await
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
