use async_trait::async_trait;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive, Selectable};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::mysql::MysqlDB;
use crate::mysql::types::AnyMysqlType;
use crate::primitives::identifier::ident;
use vantage_expressions::expr_any;

/// Create an AnyMysqlType for an id value. Always binds as string to
/// preserve semantics of textual ids (e.g., leading zeros in "00123").
/// MySQL will coerce to integer when the column type requires it.
fn id_value(id: &str) -> AnyMysqlType {
    AnyMysqlType::from(id.to_string())
}

/// Parse the JSON array result from execute() into an IndexMap of id -> Record.
fn parse_rows(
    result: AnyMysqlType,
    id_field_name: &str,
) -> Result<IndexMap<String, Record<AnyMysqlType>>> {
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

        let record: Record<AnyMysqlType> = obj
            .into_iter()
            .map(|(k, v)| (k, AnyMysqlType::untyped(v)))
            .collect();

        records.insert(id, record);
    }

    Ok(records)
}

#[async_trait]
impl TableSource for MysqlDB {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnyMysqlType;
    type Value = AnyMysqlType;
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
        let conditions: Vec<Expression<AnyMysqlType>> = table
            .columns()
            .values()
            .map(|col| {
                let p = pattern.clone();
                mysql_expr!("{} LIKE {} ESCAPE '$'", (ident(col.name())), p)
            })
            .collect();

        if conditions.is_empty() {
            return mysql_expr!("FALSE");
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

        let condition = {
            let id_val = id_value(id);
            mysql_expr!("{} = {}", (ident(&id_field_name)), id_val)
        };
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
        let result = self.aggregate(&select, "count", mysql_expr!("*")).await?;
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

        let insert = crate::mysql::statements::MysqlInsert::new(table.table_name())
            .with_field(&id_field_name, id_value(id))
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

        // MySQL: INSERT ... ON DUPLICATE KEY UPDATE ...
        let insert = crate::mysql::statements::MysqlInsert::new(table.table_name())
            .with_field(&id_field_name, id_value(id))
            .with_record(record);
        let base = insert.expr();

        let set_parts: Vec<Expression<AnyMysqlType>> = if record.is_empty() {
            vec![expr_any!(
                "{} = {}",
                (ident(&id_field_name)),
                (ident(&id_field_name))
            )]
        } else {
            record
                .keys()
                .map(|k| expr_any!("{} = VALUES({})", (ident(k)), (ident(k))))
                .collect()
        };
        let conflict = Expression::from_vec(set_parts, ", ");
        let upsert = expr_any!("{} ON DUPLICATE KEY UPDATE {}", (base), (conflict));
        self.execute(&upsert).await?;

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

        let id_condition = {
            let id_val = id_value(id);
            mysql_expr!("{} = {}", (ident(&id_field_name)), id_val)
        };
        let update = crate::mysql::statements::MysqlUpdate::new(table.table_name())
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

        let id_condition = {
            let id_val = id_value(id);
            mysql_expr!("{} = {}", (ident(&id_field_name)), id_val)
        };
        let delete = crate::mysql::statements::MysqlDelete::new(table.table_name())
            .with_condition(id_condition);
        self.execute(&delete.expr()).await?;
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let delete = crate::mysql::statements::MysqlDelete::new(table.table_name());
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
        let insert =
            crate::mysql::statements::MysqlInsert::new(table.table_name()).with_record(record);

        // MySQL doesn't support RETURNING. Execute INSERT and SELECT LAST_INSERT_ID()
        // on the same connection to get the auto-generated id.
        use crate::mysql::row::bind_mysql_value;
        use vantage_expressions::{ExpressionFlattener, Flatten};

        let expr = insert.expr();
        let flattener = ExpressionFlattener::new();
        let flattened = flattener.flatten(&expr);

        // Build the INSERT query with ? placeholders
        let template_parts: Vec<&str> = flattened.template.split("{}").collect();
        if template_parts.len() != flattened.parameters.len() + 1 {
            return Err(error!(
                "MySQL insert expression placeholder mismatch",
                placeholders = (template_parts.len() - 1).to_string(),
                parameters = flattened.parameters.len().to_string()
            ));
        }

        let mut sql = String::new();
        let mut params = Vec::new();
        sql.push_str(template_parts[0]);
        for (i, param) in flattened.parameters.iter().enumerate() {
            match param {
                ExpressiveEnum::Scalar(value) => {
                    sql.push('?');
                    params.push(value.clone());
                }
                _ => {
                    return Err(error!(
                        "MySQL insert expression contains non-scalar parameter",
                        index = i.to_string()
                    ));
                }
            }
            sql.push_str(template_parts[i + 1]);
        }

        // Acquire a single connection to ensure LAST_INSERT_ID() works
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(|e| error!("MySQL acquire connection failed", details = e.to_string()))?;

        let mut query = sqlx::query(&sql);
        for value in &params {
            query = bind_mysql_value(query, value);
        }
        query
            .execute(&mut *conn)
            .await
            .map_err(|e| error!("MySQL insert failed", details = e.to_string()))?;

        let last_id_sql = "SELECT LAST_INSERT_ID() AS id";
        let row = sqlx::query(last_id_sql)
            .fetch_one(&mut *conn)
            .await
            .map_err(|e| error!("MySQL LAST_INSERT_ID failed", details = e.to_string()))?;

        use sqlx::Row;
        let id: u64 = row
            .try_get("id")
            .map_err(|e| error!("MySQL get id failed", details = e.to_string()))?;

        Ok(id.to_string())
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
