use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive, Selectable};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::operation::Operation;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::primitives::identifier::ident;
use crate::sqlite::SqliteDB;
use crate::sqlite::types::AnySqliteType;
use vantage_expressions::expr_any;

/// Parse the JSON array result from execute() into an IndexMap of id → Record.
fn parse_rows(
    result: AnySqliteType,
    id_field_name: &str,
) -> Result<IndexMap<String, Record<AnySqliteType>>> {
    let arr = match result.into_value() {
        serde_json::Value::Array(arr) => arr,
        other => return Err(error!("expected array result", details = other.to_string())),
    };

    let mut records = IndexMap::new();
    for item in arr {
        let obj = match item {
            serde_json::Value::Object(map) => map,
            _ => continue,
        };

        let id = obj
            .get(id_field_name)
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                _ => None,
            })
            .ok_or_else(|| error!("row missing id field", field = id_field_name))?;

        let record: Record<AnySqliteType> = obj
            .into_iter()
            .map(|(k, v)| (k, AnySqliteType::untyped(v)))
            .collect();

        records.insert(id, record);
    }

    Ok(records)
}

#[async_trait]
impl TableSource for SqliteDB {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnySqliteType;
    type Value = AnySqliteType;
    type Id = String;
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
        table: &Table<Self, E>,
        search_value: &str,
    ) -> Expression<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let escaped = search_value
            .replace('$', "$$")
            .replace('%', "$%")
            .replace('_', "$_");
        let pattern = format!("%{}%", escaped);
        let conditions: Vec<Expression<AnySqliteType>> = table
            .columns()
            .values()
            .map(|col| {
                let p = pattern.clone();
                sqlite_expr!("{} LIKE {} ESCAPE '$'", (ident(col.name())), p)
            })
            .collect();

        if conditions.is_empty() {
            return sqlite_expr!("0");
        }
        Expression::from_vec(conditions, " OR ")
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

        parse_rows(result, &id_field_name)
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        let id_field_name = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let id_val = id.clone();
        let condition = sqlite_expr!("{} = {}", (ident(&id_field_name)), id_val);
        let select = table.select().with_condition(condition);
        let result = self.execute(&select.expr()).await?;

        let mut rows = parse_rows(result, &id_field_name)?;
        rows.swap_remove(id)
            .ok_or_else(|| error!("get_table_value: no row found", id = id.clone()))
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
    {
        let id_field_name = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let mut select = table.select();
        select.set_limit(Some(1), None);
        let result = self.execute(&select.expr()).await?;

        let mut rows = parse_rows(result, &id_field_name)?;
        Ok(rows.swap_remove_index(0))
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        let select = table.select();
        let result = self.aggregate(&select, "count", sqlite_expr!("*")).await?;
        result.try_get::<i64>().ok_or_else(|| {
            error!(
                "get_table_count: expected i64",
                result = format!("{}", result)
            )
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
        self.aggregate(&table.select(), "sum", column.expr()).await
    }

    async fn get_table_max<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        self.aggregate(&table.select(), "max", column.expr()).await
    }

    async fn get_table_min<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        self.aggregate(&table.select(), "min", column.expr()).await
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
        let id_field_name = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let insert = crate::sqlite::statements::SqliteInsert::new(table.table_name())
            .with_field(&id_field_name, AnySqliteType::from(id.clone()))
            .with_record(record);
        self.execute(&insert.expr()).await?;

        self.get_table_value(table, id).await
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
        let id_field_name = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        // SQLite INSERT OR REPLACE handles both insert and update
        let insert = crate::sqlite::statements::SqliteInsert::new(table.table_name())
            .with_field(&id_field_name, AnySqliteType::from(id.clone()))
            .with_record(record);
        // Rewrite as INSERT OR REPLACE
        let expr = insert.expr();
        let replace_expr = Expression::new(
            expr.template
                .replacen("INSERT INTO", "INSERT OR REPLACE INTO", 1),
            expr.parameters.clone(),
        );
        self.execute(&replace_expr).await?;

        self.get_table_value(table, id).await
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
        let id_field_name = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let id_val = id.clone();
        let id_condition = sqlite_expr!("{} = {}", (ident(&id_field_name)), id_val);
        let update = crate::sqlite::statements::SqliteUpdate::new(table.table_name())
            .with_record(partial)
            .with_condition(id_condition);
        self.execute(&update.expr()).await?;

        self.get_table_value(table, id).await
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let id_field_name = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let id_val = id.clone();
        let id_condition = sqlite_expr!("{} = {}", (ident(&id_field_name)), id_val);
        let delete = crate::sqlite::statements::SqliteDelete::new(table.table_name())
            .with_condition(id_condition);
        self.execute(&delete.expr()).await?;
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let delete = crate::sqlite::statements::SqliteDelete::new(table.table_name());
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
        let id_field_name = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());

        let insert =
            crate::sqlite::statements::SqliteInsert::new(table.table_name()).with_record(record);

        // Append RETURNING id_field to get the generated ID back
        let base = insert.expr();
        let returning = expr_any!("{} RETURNING {}", (base), (ident(&id_field_name)));
        let result = self.execute(&returning).await?;
        let mut rows = parse_rows(result, &id_field_name)?;
        rows.swap_remove_index(0)
            .map(|(id, _)| id)
            .ok_or_else(|| error!("insert_table_return_id_value: no id returned"))
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
        target_table: &str,
        target_field: &str,
        source_table: &str,
        source_column: &str,
    ) -> Self::Condition {
        sqlite_expr!(
            "{} = {}",
            (ident(target_field).dot_of(target_table)),
            (ident(source_column).dot_of(source_table))
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
        select.clear_fields();
        select.clear_order_by();
        select.add_field(column.name());

        let subquery = select.expr();
        AssociatedExpression::new(subquery, self)
    }
}
