use crate::mysql::operation::MysqlOperation;
use async_trait::async_trait;
use ciborium::Value as CborValue;
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
use crate::table_writes::{
    held_outside, new_row_not_in_set, not_in_set, patch_leaves_set, patch_not_found,
};
use vantage_expressions::expr_any;

/// Create an AnyMysqlType for an id value. Always binds as string to
/// preserve semantics of textual ids (e.g., leading zeros in "00123").
/// MySQL will coerce to integer when the column type requires it.
pub(super) fn id_value(id: &str) -> AnyMysqlType {
    AnyMysqlType::from(id.to_string())
}

/// Parse the CBOR array result from execute() into an IndexMap of id -> Record.
fn parse_rows(
    result: AnyMysqlType,
    id_field_name: &str,
) -> Result<IndexMap<String, Record<AnyMysqlType>>> {
    let arr = match result.into_value() {
        CborValue::Array(arr) => arr,
        other => {
            return Err(error!(
                "expected array result",
                details = format!("{:?}", other)
            ));
        }
    };

    let mut records = IndexMap::new();
    for item in arr {
        let map = match item {
            CborValue::Map(map) => map,
            _ => continue,
        };

        let mut id_seen = false;
        let mut id = None;
        let mut record = Record::new();
        for (k, v) in map {
            let key = match k {
                CborValue::Text(s) => s,
                _ => continue,
            };
            if key == id_field_name {
                id_seen = true;
                // A SQL NULL id (e.g. a table whose id column has neither a
                // DEFAULT nor AUTO_INCREMENT, so an omitted id inserts NULL) is
                // not a usable key: leave it `None` so the row is skipped rather
                // than keyed under the literal "Null" — which would also collide
                // every such row onto a single map entry.
                id = match &v {
                    CborValue::Text(s) => Some(s.clone()),
                    CborValue::Integer(i) => Some(i128::from(*i).to_string()),
                    CborValue::Float(f) => Some(f.to_string()),
                    CborValue::Null => None,
                    other => Some(format!("{other:?}")),
                };
            }
            record.insert(key, AnyMysqlType::untyped(v));
        }

        // A row that never carried the id column at all is a query-construction
        // bug worth surfacing; a present-but-NULL id is degenerate data we skip.
        if !id_seen {
            return Err(error!("row missing id field", field = id_field_name));
        }
        let Some(id) = id else { continue };
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
    type Condition = crate::condition::MysqlCondition;
    type Source = vantage_table::source::SelectSource<crate::mysql::statements::MysqlSelect>;

    fn eq_value_condition(&self, field: &str, value: Self::Value) -> Result<Self::Condition> {
        let column: Column<AnyMysqlType> = Column::new(field);
        Ok(MysqlOperation::eq(&column, value))
    }

    fn condition_equality(&self, condition: &Self::Condition) -> Option<(String, Self::Value)> {
        vantage_table::conditions::literal_equality(&condition.0)
    }

    fn can_confine_writes(&self) -> bool {
        true
    }

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
        let pattern = crate::like_pattern_str(search_value);
        let conditions: Vec<Expression<AnyMysqlType>> = table
            .columns()
            .values()
            // Imported implicit-reference columns exist only as projection
            // aliases — a WHERE on the dotted identifier errors at fetch time.
            .filter(|col| !table.is_imported_column(col.name()))
            .map(|col| {
                let p = pattern.clone();
                mysql_expr!("{} LIKE {} ESCAPE '$'", (ident(col.name())), p)
            })
            .collect();

        // Add each branch with `or_`. The accumulator stays a group, and
        // thus the branches stay flat: `(a OR b OR c)`, not
        // `((a OR b) OR c)`. The brackets of the group keep the other
        // conditions of the table when WHERE joins them with AND.
        let mut branches = conditions.into_iter();
        let Some(first) = branches.next() else {
            // The table has no columns to search. Match no rows.
            return mysql_expr!("FALSE").into();
        };
        let Some(second) = branches.next() else {
            // One branch needs no group.
            return first.into();
        };
        let mut group = crate::primitives::or_(first, second);
        for branch in branches {
            group = group.or_(branch);
        }
        group.expr().into()
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        let id_field_name = table.id_field_name();

        let select = table.select();
        let result = self.execute(&select.expr()).await?;

        parse_rows(result, &id_field_name)
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Option<Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        let id_field_name = table.id_field_name();

        let condition = {
            let id_val = id_value(id);
            mysql_expr!("{} = {}", (ident(&id_field_name)), id_val)
        };
        let select = table.select().with_condition(condition);
        let result = self.execute(&select.expr()).await?;

        let mut rows = parse_rows(result, &id_field_name)?;
        Ok(rows.swap_remove(id))
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
    {
        let id_field_name = table.id_field_name();

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
        // In the set already: the insert is a no-op that returns the stored row.
        if let Some(existing) = self.get_table_value(table, id).await? {
            return Ok(existing);
        }
        let id_field_name = table.id_field_name();
        if table.conditions().next().is_some() {
            if self.id_exists_anywhere(table, id).await? {
                return Err(held_outside(table, id));
            }
            let mut row = record.clone();
            row.insert(id_field_name.clone(), id_value(id));
            if !self.row_in_set(table, &row).await? {
                return Err(not_in_set(table, id));
            }
        }

        let insert = crate::mysql::statements::MysqlInsert::new(table.table_name())
            .with_record(record)
            // The explicit id param is authoritative on this path, so apply it
            // after the record — a record-carried id (e.g. one a generator hook
            // filled) must not override the id the caller asked to write.
            .with_field(&id_field_name, id_value(id));
        if let Err(e) = self.execute(&insert.expr()).await {
            // Another writer took the id between the check and the insert.
            if let Some(existing) = self.get_table_value(table, id).await? {
                return Ok(existing);
            }
            if self.id_exists_anywhere(table, id).await? {
                return Err(held_outside(table, id));
            }
            return Err(e);
        }

        self.get_table_value(table, id)
            .await?
            .ok_or_else(|| error!("Inserted row disappeared", id = id.clone()))
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
        let id_field_name = table.id_field_name();
        if table.conditions().next().is_some() {
            let in_set = self.get_table_value(table, id).await?.is_some();
            if !in_set && self.id_exists_anywhere(table, id).await? {
                return Err(held_outside(table, id));
            }
            let mut row = record.clone();
            row.insert(id_field_name.clone(), id_value(id));
            if !self.row_in_set(table, &row).await? {
                return Err(not_in_set(table, id));
            }
        }

        // MySQL: INSERT ... ON DUPLICATE KEY UPDATE ...
        let insert = crate::mysql::statements::MysqlInsert::new(table.table_name())
            .with_record(record)
            // The explicit id param is authoritative on this path, so apply it
            // after the record — a record-carried id (e.g. one a generator hook
            // filled) must not override the id the caller asked to write.
            .with_field(&id_field_name, id_value(id));
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

        self.get_table_value(table, id)
            .await?
            .ok_or_else(|| error!("Row missing after upsert", id = id.clone()))
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
        let Some(current) = self.get_table_value(table, id).await? else {
            return Err(patch_not_found(table, id));
        };
        if table.conditions().next().is_some() {
            let mut merged = current;
            for (k, v) in partial.iter() {
                merged.insert(k.clone(), v.clone());
            }
            if !self.row_in_set(table, &merged).await? {
                return Err(patch_leaves_set(table, id));
            }
        }

        let id_field_name = table.id_field_name();
        let id_condition = {
            let id_val = id_value(id);
            mysql_expr!("{} = {}", (ident(&id_field_name)), id_val)
        };
        let mut update = crate::mysql::statements::MysqlUpdate::new(table.table_name())
            .with_record(partial)
            .with_condition(id_condition);
        for condition in table.conditions() {
            update = update.with_condition(condition.clone());
        }
        self.execute(&update.expr()).await?;

        crate::table_writes::refetch_after_patch(self, table, id).await
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let id_field_name = table.id_field_name();

        let id_condition = {
            let id_val = id_value(id);
            mysql_expr!("{} = {}", (ident(&id_field_name)), id_val)
        };
        // A missing row, or one outside the conditions, matches nothing: Ok.
        let mut delete = crate::mysql::statements::MysqlDelete::new(table.table_name())
            .with_condition(id_condition);
        for condition in table.conditions() {
            delete = delete.with_condition(condition.clone());
        }
        self.execute(&delete.expr()).await?;
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        // A conditioned table is a subset — see the trait's contract.
        let mut delete = crate::mysql::statements::MysqlDelete::new(table.table_name());
        for condition in table.conditions() {
            delete = delete.with_condition(condition.clone());
        }
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
        if !self.row_in_set(table, record).await? {
            return Err(new_row_not_in_set(table));
        }
        let insert =
            crate::mysql::statements::MysqlInsert::new(table.table_name()).with_record(record);

        // MySQL doesn't support RETURNING. Execute INSERT and SELECT LAST_INSERT_ID()
        // on the same connection to get the auto-generated id.
        let (sql, params) = crate::sql_exec::prepare(
            &insert.expr(),
            <Self as crate::sql_exec::SqlDialect>::PLACEHOLDER,
        )
        .await?;

        // Acquire a single connection to ensure LAST_INSERT_ID() works
        let mut conn = self
            .pool()
            .acquire()
            .await
            .map_err(|e| error!("MySQL acquire connection failed", details = e.to_string()))?;

        crate::sql_exec::bind_all::<Self>(&sql, &params)?
            .execute(&mut *conn)
            .await
            .map_err(|e| {
                let err = error!("MySQL insert failed", details = e.to_string());
                crate::sql_exec::failure_context::<Self>(err, &sql, &params)
            })?;

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
        mysql_expr!(
            "{} = {}",
            (ident(target_field).dot_of(target_table)),
            (ident(source_column).dot_of(source_table))
        )
        .into()
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
