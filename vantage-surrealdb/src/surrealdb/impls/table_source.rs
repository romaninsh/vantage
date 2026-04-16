use async_trait::async_trait;
use indexmap::IndexMap;

use crate::operation::SurrealOperation;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::identifier::Identifier;
use crate::statements::delete::SurrealDelete;
use crate::statements::insert::SurrealInsert;
use crate::statements::update::SurrealUpdate;

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
        match AnySurrealType::from_cbor(&v) {
            Some(val) => {
                fields.insert(key, val);
            }
            None => {
                eprintln!(
                    "parse_cbor_row: dropping field '{}', unsupported CBOR: {:?}",
                    key, v
                );
            }
        }
    }

    (thing, Record::from_indexmap(fields))
}

/// Extract the first CBOR map from a result that may be a map or an array-of-maps.
fn extract_first_map(
    result: AnySurrealType,
) -> vantage_dataset::traits::Result<Vec<(ciborium::Value, ciborium::Value)>> {
    let value = result.into_value();
    match value {
        ciborium::Value::Map(m) => Ok(m),
        ciborium::Value::Array(arr) => arr
            .into_iter()
            .find_map(|v| match v {
                ciborium::Value::Map(m) => Some(m),
                _ => None,
            })
            .ok_or_else(|| error!("expected map in array result")),
        _ => Err(error!("expected map or array result")),
    }
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
    type Condition = vantage_expressions::Expression<Self::Value>;

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
    ) -> Expression<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        // TODO: iterate searchable columns
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

        let select = table.select();
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
        let mut select = table.select();
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

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        let mut select = table.select();
        select.order_by.clear(); // ordering is unnecessary for count
        let count_query = select.as_count();
        let result = self.execute(&count_query.expr()).await?;
        result.try_get::<i64>().ok_or_else(|| {
            vantage_core::error!("get_count: expected i64", result = format!("{}", result))
        })
    }

    async fn get_table_sum<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let mut select = table.select();
        select.order_by.clear();
        let sum_query = select.as_sum(column.clone());
        self.execute(&sum_query.expr()).await
    }

    async fn get_table_max<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let mut select = table.select();
        select.order_by.clear();
        let max_query = select.as_max(column.clone());
        self.execute(&max_query.expr()).await
    }

    async fn get_table_min<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let mut select = table.select();
        select.order_by.clear();
        let min_query = select.as_min(column.clone());
        self.execute(&min_query.expr()).await
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
        let mut insert = SurrealInsert::new(table.table_name()).with_id(id.id());
        for (key, value) in record.iter() {
            insert = insert.with_any_field(key, value.clone());
        }
        let result = self.execute(&insert.expr()).await?;
        let map = extract_first_map(result)?;
        let id_field = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());
        let (_thing, rec) = parse_cbor_row(map, &id_field);
        Ok(rec)
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
        let update = SurrealUpdate::new(id.clone()).content().with_record(record);
        let result = self.execute(&update.expr()).await?;
        let map = extract_first_map(result)?;
        let id_field = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());
        let (_thing, rec) = parse_cbor_row(map, &id_field);
        Ok(rec)
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
        let update = SurrealUpdate::new(id.clone()).merge().with_record(partial);
        let result = self.execute(&update.expr()).await?;
        let map = extract_first_map(result)?;
        let id_field = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());
        let (_thing, rec) = parse_cbor_row(map, &id_field);
        Ok(rec)
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let delete = SurrealDelete::new(id.clone());
        self.execute(&delete.expr()).await?;
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let delete = SurrealDelete::table(table.table_name());
        self.execute(&delete.expr()).await?;
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
        let mut insert = SurrealInsert::new(table.table_name());
        for (key, value) in record.iter() {
            insert = insert.with_any_field(key, value.clone());
        }
        // Append RETURN id
        let base = insert.expr();
        let query = Expression::new(format!("{} RETURN id", base.template), base.parameters);
        let result = self.execute(&query).await?;
        let map = extract_first_map(result)?;
        let (thing, _rec) = parse_cbor_row(map, "id");
        thing.ok_or_else(|| error!("insert_table_return_id_value: no id returned"))
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
        let src_col = self.create_column::<Self::AnyType>(source_column);
        let fk_values = self.column_table_values_expr(source_table, &src_col);
        let tgt_col = self.create_column::<Self::AnyType>(target_field);
        tgt_col.in_(fk_values.expr())
    }

    fn related_correlated_condition(
        &self,
        _target_table: &str,
        target_field: &str,
        _source_table: &str,
        source_column: &str,
    ) -> Self::Condition {
        use crate::identifier::Parent;
        crate::surreal_expr!(
            "{} = {}",
            (Identifier::new(target_field)),
            (Parent::identifier().dot(source_column))
        )
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
        let mut select = table.select();
        select.order_by.clear();
        select.fields.clear();

        use crate::select::select_field::SelectField;
        select
            .fields
            .push(SelectField::new(Identifier::new(column.name())));

        let select = select.with_value();
        let expr = select.expr();

        let deferred_expr = Expression::new("{}", vec![ExpressiveEnum::Deferred(self.defer(expr))]);
        AssociatedExpression::new(deferred_expr, self)
    }
}
