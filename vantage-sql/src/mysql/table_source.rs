use crate::mysql::operation::MysqlOperation;
use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use sqlx::Row;
use sqlx::mysql::MySqlConnection;
use vantage_core::{Result, error};
use vantage_dataset::Inserted;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_expressions::{Expression, Expressive, Selectable, expr_any};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::sorting::SortDirection;
use vantage_table::table::{Aggregate, Table};
use vantage_table::traits::column_like::ColumnLike as _;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};
use vantage_vista::Transaction;

use crate::declared::{sql_coerce_fn, sql_declared_types};
use crate::mysql::MysqlDB;
use crate::mysql::statements::MysqlSelect;
use crate::mysql::types::{AnyMysqlType, MysqlType};
use crate::primitives::identifier::ident;
use crate::set_writes::{find_by_record, id_exists_anywhere, id_target};
use crate::sql_exec::mark_transport;
use crate::table_writes::{
    existing_by_record, guard_insert, guard_new_row, guard_patch, guard_replace, null_derived_id,
    settle_failed_derived_insert, settle_failed_insert,
};

sql_declared_types!(sql_coerce_fn!(MysqlDB, MysqlType, AnyMysqlType));

/// Render a select for a read that returns rows: `FOR UPDATE` after the limit
/// when the table is locked.
fn row_query<E>(table: &Table<MysqlDB, E>, select: &MysqlSelect) -> Expression<AnyMysqlType>
where
    E: Entity<AnyMysqlType>,
{
    if table.is_locked() {
        Expression::new("{} FOR UPDATE", vec![ExpressiveEnum::Nested(select.expr())])
    } else {
        select.expr()
    }
}

/// Create an AnyMysqlType for an id value. Always binds as string to
/// preserve semantics of textual ids (e.g., leading zeros in "00123").
/// MySQL will coerce to integer when the column type requires it.
pub(super) fn id_value(id: &str) -> AnyMysqlType {
    AnyMysqlType::from(id.to_string())
}

/// `INSERT` of `record` under `id`. The id param is applied after the record:
/// a record-carried id (e.g. one a generator hook filled) must not override
/// the id the caller asked to write.
fn insert_with_id<E: Entity<AnyMysqlType>>(
    table: &Table<MysqlDB, E>,
    id: &str,
    record: &Record<AnyMysqlType>,
) -> Expression<AnyMysqlType> {
    crate::mysql::statements::MysqlInsert::new(table.table_name())
        .with_record(record)
        .with_field(table.id_field_name(), id_value(id))
        .expr()
}

/// `UPDATE … SET <values> WHERE id = ? AND <the table's conditions>`.
fn update_in_set<E: Entity<AnyMysqlType>>(
    table: &Table<MysqlDB, E>,
    id: &str,
    values: &Record<AnyMysqlType>,
) -> Result<Expression<AnyMysqlType>> {
    let id_val = id_value(id);
    let id_condition = mysql_expr!("{} = {}", (id_target(table)?), id_val);
    let mut update = crate::mysql::statements::MysqlUpdate::new(table.table_name())
        .with_record(values)
        .with_condition(id_condition);
    for condition in table.conditions() {
        update = update.with_condition(condition.clone());
    }
    Ok(update.expr())
}

/// The rows of a read, keyed by id. A `json` column's JSON text reads as the
/// map or array it holds.
pub(super) fn parse_rows<E: Entity<AnyMysqlType>>(
    table: &Table<MysqlDB, E>,
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
            let v = match table.columns().get(&key) {
                Some(declared) => crate::types::read_json_cell(
                    table.table_name(),
                    &key,
                    crate::declared::without_option(declared.get_type()),
                    v,
                )?,
                None => v,
            };
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

/// Run the INSERT, then read the id it generated on the same connection.
async fn insert_returning_id(
    conn: &mut MySqlConnection,
    sql: &str,
    params: &[AnyMysqlType],
) -> Result<String> {
    crate::sql_exec::bind_all::<MysqlDB>(sql, params)?
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            let err = mark_transport(error!("MySQL insert failed", details = e.to_string()), &e);
            crate::sql_exec::failure_context::<MysqlDB>(err, sql, params)
        })?;

    // The row is written by now: a lost connection here must not invite a
    // retry that would insert it again under a new id.
    let row = sqlx::query("SELECT LAST_INSERT_ID() AS id")
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| error!("MySQL LAST_INSERT_ID failed", details = e.to_string()))?;

    let id: u64 = row
        .try_get("id")
        .map_err(|e| error!("MySQL get id failed", details = e.to_string()))?;

    Ok(id.to_string())
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

    fn in_values_condition(
        &self,
        field: &str,
        values: Vec<Self::Value>,
    ) -> Result<Self::Condition> {
        let column: Column<AnyMysqlType> = Column::new(field);
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
        // MySQL drops a derived table's ORDER BY unless it has a LIMIT, so
        // the rows are re-ordered under one; `GROUP_CONCAT` orders itself.
        let by = crate::list_aggregate::order_list(order);
        Ok(match aggregate {
            Aggregate::JoinConcat { separator, .. } => {
                let separator = crate::list_aggregate::mysql_literal(separator)?;
                expr_any!(
                    &format!(
                        "(SELECT COALESCE(GROUP_CONCAT(v ORDER BY {by} SEPARATOR {separator}), '') \
                         FROM ({{}}) AS r)"
                    ),
                    (rows)
                )
            }
            Aggregate::JoinJson(_) => expr_any!(
                &format!(
                    "(SELECT COALESCE(JSON_ARRAYAGG(v), JSON_ARRAY()) FROM \
                     (SELECT * FROM ({{}}) AS q ORDER BY {by} LIMIT 18446744073709551615) AS r)"
                ),
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
        value: AnyMysqlType,
    ) -> Result<AnyMysqlType>
    where
        E: Entity<AnyMysqlType>,
    {
        coerce_column(table, column, value)
    }

    fn contained_records(&self, host: &AnyMysqlType) -> Result<Option<Vec<Record<AnyMysqlType>>>> {
        crate::types::contained_records(host.value(), AnyMysqlType::untyped)
    }

    fn contained_host(
        &self,
        records: Vec<Record<AnyMysqlType>>,
        many: bool,
    ) -> Result<AnyMysqlType> {
        Ok(AnyMysqlType::untyped(crate::types::contained_host(
            records,
            many,
            AnyMysqlType::into_value,
        )))
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
        let conditions: Vec<Expression<AnyMysqlType>> = table
            .search_columns()
            .into_iter()
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

        let select = table.try_select()?;
        let result = self.execute(&row_query(table, &select)).await?;

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

        let condition = {
            let id_val = id_value(id);
            mysql_expr!("{} = {}", (id_target(table)?), id_val)
        };
        let select = table.try_select()?.with_condition(condition);
        let result = self.execute(&row_query(table, &select)).await?;

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
        let result = self.execute(&row_query(table, &select)).await?;

        let mut rows = parse_rows(table, result, &id_field_name)?;
        Ok(rows.swap_remove_index(0))
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        let select = table.select_empty();
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

        if let Err(e) = self.execute(&insert_with_id(table, id, record)).await {
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
        // Not `ON DUPLICATE KEY UPDATE`: that fires on any UNIQUE column and
        // would update whichever row holds the same e.g. email. Look the id
        // up instead and UPDATE or INSERT by id alone; another UNIQUE clash,
        // or a concurrent insert of the same id, is a duplicate-key error.
        // Columns the record leaves out keep their stored values.
        if id_exists_anywhere(self, table, id).await? {
            // The explicit id param is authoritative: a record-carried id
            // must not rewrite the row's key.
            let mut values = record.clone();
            values.shift_remove(&id_field_name);
            if !values.is_empty() {
                self.execute(&update_in_set(table, id, &values)?).await?;
            }
        } else {
            self.execute(&insert_with_id(table, id, record)).await?;
        }

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
        guard_patch(self, table, id, partial).await?;
        self.execute(&update_in_set(table, id, partial)?).await?;

        crate::table_writes::refetch_after_patch(self, table, id).await
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let id_condition = {
            let id_val = id_value(id);
            mysql_expr!("{} = {}", (id_target(table)?), id_val)
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
        guard_new_row(self, table, record).await?;
        let insert =
            crate::mysql::statements::MysqlInsert::new(table.table_name()).with_record(record);

        // MySQL doesn't support RETURNING. Execute INSERT and SELECT LAST_INSERT_ID()
        // on the same connection to get the auto-generated id.
        let (sql, params) = crate::sql_exec::prepare(
            &insert.expr(),
            <Self as crate::sql_exec::SqlDialect>::PLACEHOLDER,
        )
        .await?;

        // Both statements run on one connection, for LAST_INSERT_ID(): the
        // transaction's, when there is one.
        let Some(tx) = &self.tx else {
            let mut conn = self.pool().acquire().await.map_err(|e| {
                mark_transport(
                    error!("MySQL acquire connection failed", details = e.to_string()),
                    &e,
                )
            })?;
            return insert_returning_id(&mut conn, &sql, &params).await;
        };
        let mut open = tx.open().await?;
        tx.watch(insert_returning_id(&mut open, &sql, &params))
            .await
    }

    /// MySQL has no `RETURNING`: the INSERT, then the row read back by the
    /// record's values, the same lookup a failed insert settles by.
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
        let insert =
            crate::mysql::statements::MysqlInsert::new(table.table_name()).with_record(record);
        if let Err(e) = self.execute(&insert.expr()).await {
            return settle_failed_derived_insert(self, table, record, e).await;
        }
        // The row is read back by its values, which must single it out: a
        // NULL id, or a column the record leaves to the database, can't.
        let (id, stored) = find_by_record(self, table, record)
            .await?
            .ok_or_else(|| null_derived_id(table))?;
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
