use std::any::type_name;

use crate::sqlite::operation::SqliteOperation;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::Inserted;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive, Selectable};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::sorting::SortDirection;
use vantage_table::table::{Aggregate, Table};
use vantage_table::traits::column_like::ColumnLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};
use vantage_vista::Transaction;

use crate::declared::{sql_coerce_fn, sql_declared_types, without_option};
use crate::primitives::identifier::ident;
use crate::set_writes::{SetWrites as _, id_target};
use crate::sqlite::SqliteDB;
use crate::sqlite::types::{AnySqliteType, SqliteType};
use crate::table_writes::{
    existing_by_record, guard_insert, guard_new_row, guard_patch, guard_replace, null_derived_id,
    settle_failed_derived_insert, settle_failed_insert, upsert_on_id,
};
use vantage_expressions::expr_any;

sql_declared_types!(sql_coerce_fn!(SqliteDB, SqliteType, AnySqliteType));

/// A stored cell as `column` of `table` declares it: the read side of
/// `coerce_column`. SQLite keeps a date, time, timestamp or `json` value as
/// text, which its storage class can't tell from a string, so the declared
/// type decides; a `json` cell reads as the map or array it holds. Every
/// other cell stays as the row decoded it.
fn read_column<E: Entity<AnySqliteType>>(
    table: &Table<SqliteDB, E>,
    column: &str,
    value: CborValue,
) -> Result<AnySqliteType> {
    let CborValue::Text(text) = &value else {
        return Ok(AnySqliteType::untyped(value));
    };
    let Some(declared) = table.columns().get(column) else {
        return Ok(AnySqliteType::untyped(value));
    };
    let declared_type = without_option(declared.get_type());
    let read = if declared_type == type_name::<NaiveDate>() {
        stored::<NaiveDate>(text).ok_or("date")
    } else if declared_type == type_name::<NaiveTime>() {
        stored::<NaiveTime>(text).ok_or("time")
    } else if declared_type == type_name::<NaiveDateTime>() {
        stored::<NaiveDateTime>(text).ok_or("datetime")
    } else if declared_type == type_name::<DateTime<Utc>>() {
        stored::<DateTime<Utc>>(text).ok_or("timestamp")
    } else if declared_type == type_name::<serde_json::Value>() {
        let cell = crate::types::read_json_cell(table.table_name(), column, declared_type, value)?;
        return Ok(AnySqliteType::untyped(cell));
    } else {
        return Ok(AnySqliteType::untyped(value));
    };
    read.map_err(|expected| {
        error!(
            format!(
                "`{column}` of `{}` holds {text:?}, which is not a `{expected}`",
                table.table_name()
            ),
            table = table.table_name(),
            column = column,
            expected = expected,
            value = text
        )
    })
}

/// `text` through the decode writes use, so a read takes every form a write
/// does. `""` is no value, as a write treats it.
fn stored<T: SqliteType>(text: &str) -> Option<AnySqliteType> {
    if text.is_empty() {
        return Some(AnySqliteType::untyped(CborValue::Null));
    }
    T::coerce_cbor(CborValue::Text(text.to_string())).map(AnySqliteType::new)
}

/// Parse the CBOR array result from execute() into an IndexMap of id → Record.
fn parse_rows<E: Entity<AnySqliteType>>(
    table: &Table<SqliteDB, E>,
    result: AnySqliteType,
    id_field_name: &str,
) -> Result<IndexMap<String, Record<AnySqliteType>>> {
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
                // DEFAULT nor AUTOINCREMENT, so an omitted id inserts NULL) is
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
            let cell = read_column(table, &key, v)?;
            record.insert(key, cell);
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

/// A write as `INSERT OR ABORT` / `UPDATE OR ABORT`. Without it a UNIQUE
/// column the schema declares `ON CONFLICT REPLACE` deletes the row that
/// clashes with the write, and `ON CONFLICT IGNORE` drops the write silently.
fn or_abort(expr: Expression<AnySqliteType>) -> Expression<AnySqliteType> {
    let template = if let Some(rest) = expr.template.strip_prefix("INSERT ") {
        format!("INSERT OR ABORT {rest}")
    } else if let Some(rest) = expr.template.strip_prefix("UPDATE ") {
        format!("UPDATE OR ABORT {rest}")
    } else {
        return expr;
    };
    Expression::new(template, expr.parameters)
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
    type Condition = crate::condition::SqliteCondition;
    type Source = vantage_table::source::SelectSource<crate::sqlite::statements::SqliteSelect>;

    fn eq_value_condition(&self, field: &str, value: Self::Value) -> Result<Self::Condition> {
        let column: Column<AnySqliteType> = Column::new(field);
        Ok(SqliteOperation::eq(&column, value))
    }

    fn in_values_condition(
        &self,
        field: &str,
        values: Vec<Self::Value>,
    ) -> Result<Self::Condition> {
        let column: Column<AnySqliteType> = Column::new(field);
        Ok(crate::condition::in_values(&column, &values))
    }

    fn condition_equality(&self, condition: &Self::Condition) -> Option<(String, Self::Value)> {
        vantage_table::conditions::literal_equality(&condition.0)
    }

    fn describe_condition(&self, condition: &Self::Condition) -> String {
        condition.0.preview()
    }

    fn condition_column(&self, condition: &Self::Condition) -> Option<String> {
        vantage_table::conditions::condition_column_of(&condition.0)
    }

    fn expression_condition(&self, expr: Expression<Self::Value>) -> Result<Self::Condition> {
        Ok(expr.into())
    }

    fn render_list_aggregate(
        &self,
        rows: Expression<Self::Value>,
        aggregate: &Aggregate,
        order: &[SortDirection],
    ) -> Result<Expression<Self::Value>> {
        // The derived table is in order; SQLite aggregates in scan order.
        let _ = order;
        Ok(match aggregate {
            Aggregate::JoinConcat { separator, .. } => expr_any!(
                "(SELECT COALESCE(group_concat(v, {}), '') FROM ({}) AS r)",
                (AnySqliteType::from(separator.clone())),
                (rows)
            ),
            Aggregate::JoinJson(_) => expr_any!(
                "(SELECT COALESCE(json_group_array(v), '[]') FROM ({}) AS r)",
                (rows)
            ),
            other => return Err(error!(format!("not a list aggregate: {other:?}"))),
        })
    }

    fn can_confine_writes(&self) -> bool {
        true
    }

    fn coerce_column_value<E>(
        &self,
        table: &Table<Self, E>,
        column: &str,
        value: AnySqliteType,
    ) -> Result<AnySqliteType>
    where
        E: Entity<AnySqliteType>,
    {
        coerce_column(table, column, value)
    }

    fn contained_records(
        &self,
        host: &AnySqliteType,
    ) -> Result<Option<Vec<Record<AnySqliteType>>>> {
        crate::types::contained_records(host.value(), AnySqliteType::untyped)
    }

    /// JSON text: SQLite binds no map or array, and a text or `json` host
    /// column both take the text.
    fn contained_host(
        &self,
        records: Vec<Record<AnySqliteType>>,
        many: bool,
    ) -> Result<AnySqliteType> {
        let host = crate::types::contained_host(records, many, AnySqliteType::into_value);
        Ok(AnySqliteType::untyped(match host {
            CborValue::Null => CborValue::Null,
            host => CborValue::Text(crate::types::cbor_to_json(host).to_string()),
        }))
    }

    fn transaction(&self) -> Option<Transaction> {
        self.tx.as_ref().and_then(|tx| tx.transaction())
    }

    fn without_transaction(&self) -> Self {
        Self {
            tx: None,
            ..self.clone()
        }
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
        let conditions: Vec<Expression<AnySqliteType>> = table
            .search_columns()
            .into_iter()
            // Imported implicit-reference columns exist only as projection
            // aliases; in a WHERE clause the dotted identifier degrades to a
            // string literal and silently matches wrong rows.
            .filter(|col| !table.is_imported_column(col.name()))
            .map(|col| {
                let p = pattern.clone();
                let subject = crate::search_subject(table, col.name());
                sqlite_expr!("{} LIKE {} ESCAPE '$'", (subject), p)
            })
            .collect();

        // Add each branch with `or_`. The accumulator stays a group, and
        // thus the branches stay flat: `(a OR b OR c)`, not
        // `((a OR b) OR c)`. The brackets of the group keep the other
        // conditions of the table when WHERE joins them with AND.
        let mut branches = conditions.into_iter();
        let Some(first) = branches.next() else {
            // The table has no columns to search. Match no rows.
            return sqlite_expr!("0").into();
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

        let select = table.try_select()?;
        let result = self.execute(&select.expr()).await?;

        parse_rows(table, result, &id_field_name)
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

        let id_val = Self::id_param(table, id);
        let condition = sqlite_expr!("{} = {}", (id_target(table)?), id_val);
        let select = table.try_select()?.with_condition(condition);
        let result = self.execute(&select.expr()).await?;

        let mut rows = parse_rows(table, result, &id_field_name)?;
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

        let mut select = table.try_select()?;
        select.set_limit(Some(1), None);
        let result = self.execute(&select.expr()).await?;

        let mut rows = parse_rows(table, result, &id_field_name)?;
        Ok(rows.swap_remove_index(0))
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        let select = table.select_empty();
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
        let value = table.try_column_expr_of(column)?;
        self.aggregate(&table.select_empty(), "sum", value).await
    }

    async fn get_table_max<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let value = table.try_column_expr_of(column)?;
        self.aggregate(&table.select_empty(), "max", value).await
    }

    async fn get_table_min<E>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        let value = table.try_column_expr_of(column)?;
        self.aggregate(&table.select_empty(), "min", value).await
    }

    async fn insert_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Inserted<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        if let Some(existing) = guard_insert(self, table, id, record).await? {
            return Ok(Inserted::Existed(existing));
        }

        let id_field_name = table.id_field_name();
        let insert = crate::sqlite::statements::SqliteInsert::new(table.table_name())
            .with_record(record)
            // The explicit id param is authoritative on this path, so apply it
            // after the record — a record-carried id (e.g. one a generator hook
            // filled) must not override the id the caller asked to write.
            .with_field(&id_field_name, AnySqliteType::from(id.clone()));
        if let Err(e) = self.execute(&or_abort(insert.expr())).await {
            return settle_failed_insert(self, table, id, e).await;
        }

        self.get_table_value(table, id)
            .await?
            .map(Inserted::Created)
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
        guard_replace(self, table, id, record).await?;

        let id_field_name = table.id_field_name();
        let insert = crate::sqlite::statements::SqliteInsert::new(table.table_name())
            .with_record(record)
            // The explicit id param is authoritative on this path, so apply it
            // after the record — a record-carried id (e.g. one a generator hook
            // filled) must not override the id the caller asked to write.
            .with_field(&id_field_name, AnySqliteType::from(id.clone()));
        let replace_expr = upsert_on_id(or_abort(insert.expr()), &id_field_name, record);
        self.execute(&replace_expr).await?;

        self.get_table_value(table, id)
            .await?
            .ok_or_else(|| error!("Row missing after replace", id = id.clone()))
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
        guard_patch(self, table, id, partial).await?;

        let id_val = Self::id_param(table, id);
        let id_condition = sqlite_expr!("{} = {}", (id_target(table)?), id_val);
        let mut update = crate::sqlite::statements::SqliteUpdate::new(table.table_name())
            .with_record(partial)
            .with_condition(id_condition);
        for condition in table.conditions() {
            update = update.with_condition(condition.clone());
        }
        self.execute(&or_abort(update.expr())).await?;

        crate::table_writes::refetch_after_patch(self, table, id).await
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let id_val = Self::id_param(table, id);
        let id_condition = sqlite_expr!("{} = {}", (id_target(table)?), id_val);
        // A missing row, or one outside the conditions, matches nothing: Ok.
        let mut delete = crate::sqlite::statements::SqliteDelete::new(table.table_name())
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
        let mut delete = crate::sqlite::statements::SqliteDelete::new(table.table_name());
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
        guard_new_row(self, table, record).await?;
        let id_field_name = table.id_field_name();

        let insert =
            crate::sqlite::statements::SqliteInsert::new(table.table_name()).with_record(record);

        let base = or_abort(insert.expr());
        let returning = expr_any!("{} RETURNING {}", (base), (ident(&id_field_name)));
        let result = self.execute(&returning).await?;
        let mut rows = parse_rows(table, result, &id_field_name)?;
        rows.swap_remove_index(0).map(|(id, _)| id).ok_or_else(|| {
            error!(
                "insert returned no usable id — the row's id came back NULL; \
                     does the table's id column have a DEFAULT or AUTOINCREMENT?",
                table = table.table_name()
            )
        })
    }

    /// `INSERT … RETURNING (<id expression>) AS <id>`; a failed insert
    /// settles by the record.
    async fn insert_table_derived_id_value<E>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Self::Value>,
    ) -> Result<(Self::Id, Inserted<Self::Value>)>
    where
        E: Entity<Self::Value>,
    {
        guard_new_row(self, table, record).await?;
        if let Some(existed) = existing_by_record(self, table, record).await? {
            return Ok(existed);
        }
        let id_field_name = table.id_field_name();
        let insert =
            crate::sqlite::statements::SqliteInsert::new(table.table_name()).with_record(record);
        let returning = expr_any!(
            "{} RETURNING {} AS {}",
            (or_abort(insert.expr())),
            (id_target(table)?),
            (ident(&id_field_name))
        );
        let result = match self.execute(&returning).await {
            Ok(result) => result,
            Err(e) => return settle_failed_derived_insert(self, table, record, e).await,
        };
        let Some((id, _)) = parse_rows(table, result, &id_field_name)?.swap_remove_index(0) else {
            return Err(null_derived_id(table));
        };
        let stored = self
            .get_table_value(table, &id)
            .await?
            .ok_or_else(|| error!("Inserted row disappeared", id = id.clone()))?;
        Ok((id, Inserted::Created(stored)))
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

    // Implicit references lower into the generic nested correlated-subquery
    // chain built on `related_correlated_condition` below; no native path
    // override is needed.
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
        sqlite_expr!(
            "{} = {}",
            (ident(target_field).dot_of(target_table)),
            (ident(source_column).dot_of(source_table))
        )
        .into()
    }

    fn try_related_correlated_condition(
        &self,
        target_table: &str,
        target_field: &str,
        source_table: &str,
        source_column: &str,
    ) -> Result<Self::Condition> {
        Ok(self.related_correlated_condition(
            target_table,
            target_field,
            source_table,
            source_column,
        ))
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
        // Only this column's callback runs: an inherited rule builds this
        // subquery while its own table is being built.
        let select = table
            .try_select_only(column)
            .unwrap_or_else(|e| panic!("{e}"));

        let subquery = select.expr();
        AssociatedExpression::new(subquery, self)
    }
}
