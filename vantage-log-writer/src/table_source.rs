use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::Value;
use vantage_core::{Result, error};
use vantage_expressions::Expression;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::{DataSource, ExprDataSource};
use vantage_expressions::traits::expressive::{DeferredFn, ExpressiveEnum};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::log_writer::LogWriter;
use crate::type_system::AnyJsonType;
use crate::writer_task::WriteOp;

impl DataSource for LogWriter {}

impl ExprDataSource<Value> for LogWriter {
    async fn execute(&self, _expr: &Expression<Value>) -> Result<Value> {
        Err(unsupported("execute"))
    }

    fn defer(&self, _expr: Expression<Value>) -> DeferredFn<Value>
    where
        Value: Clone + Send + Sync + 'static,
    {
        DeferredFn::new(move || {
            Box::pin(async move { Err(unsupported("defer")) })
        })
    }
}

fn unsupported(method: &'static str) -> vantage_core::VantageError {
    error!("log-writer is insert-only", method = method).is_unsupported()
}

#[async_trait]
impl TableSource for LogWriter {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnyJsonType;
    type Value = Value;
    type Id = String;
    type Condition = Expression<Self::Value>;

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
        _search_value: &str,
    ) -> Self::Condition
    where
        E: Entity<Self::Value>,
    {
        Expression::new("", vec![])
    }

    async fn list_table_values<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(unsupported("list_table_values"))
    }

    async fn get_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
    ) -> Result<Option<Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(unsupported("get_table_value"))
    }

    async fn get_table_some_value<E>(
        &self,
        _table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(unsupported("get_table_some_value"))
    }

    async fn get_table_count<E>(&self, _table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(unsupported("get_table_count"))
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
        Err(unsupported("get_table_sum"))
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
        Err(unsupported("get_table_max"))
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
        Err(unsupported("get_table_min"))
    }

    async fn insert_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let projected = project_record(table.columns().keys(), record, self.id_column(), id);
        let line = serialize_line(&projected)?;
        let path = self.file_path(table.table_name());
        self.sender()
            .send(WriteOp::Append { path, line })
            .await
            .map_err(|e| error!("log writer channel closed", detail = e.to_string()))?;
        Ok(projected)
    }

    async fn replace_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(unsupported("replace_table_value"))
    }

    async fn patch_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(unsupported("patch_table_value"))
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(unsupported("delete_table_value"))
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(unsupported("delete_table_all_values"))
    }

    async fn insert_table_return_id_value<E>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let id = extract_or_generate_id(record, self.id_column());
        let projected = project_record(table.columns().keys(), record, self.id_column(), &id);
        let line = serialize_line(&projected)?;
        let path = self.file_path(table.table_name());
        self.sender()
            .send(WriteOp::Append { path, line })
            .await
            .map_err(|e| error!("log writer channel closed", detail = e.to_string()))?;
        Ok(id)
    }

    fn related_in_condition<SourceE: Entity<Self::Value> + 'static>(
        &self,
        _target_field: &str,
        _source_table: &Table<Self, SourceE>,
        _source_column: &str,
    ) -> Self::Condition
    where
        Self: Sized,
    {
        Expression::new("", vec![])
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: Sized,
    {
        unimplemented!("log-writer is insert-only; column_table_values_expr is unreachable")
    }
}

/// Project a record onto the table's declared column set, then attach the id.
///
/// Entity fields not declared as columns are dropped — this is the contract
/// the user pinned down: "entity values with non-existent columns would be
/// dropped".
fn project_record<'a, I>(
    column_names: I,
    record: &Record<Value>,
    id_column: &str,
    id: &str,
) -> Record<Value>
where
    I: IntoIterator<Item = &'a String>,
{
    let mut out = Record::new();
    let mut wrote_id = false;
    for col in column_names {
        if col == id_column {
            out.insert(col.clone(), Value::String(id.to_string()));
            wrote_id = true;
        } else if let Some(v) = record.get(col) {
            out.insert(col.clone(), v.clone());
        }
    }
    if !wrote_id {
        out.insert(id_column.to_string(), Value::String(id.to_string()));
    }
    out
}

fn serialize_line(record: &Record<Value>) -> Result<String> {
    let map: serde_json::Map<String, Value> = record
        .as_inner()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut s = serde_json::to_string(&Value::Object(map))
        .map_err(|e| error!("failed to serialize record to JSON", detail = e.to_string()))?;
    s.push('\n');
    Ok(s)
}

fn extract_or_generate_id(record: &Record<Value>, id_column: &str) -> String {
    if let Some(v) = record.get(id_column) {
        match v {
            Value::String(s) if !s.is_empty() => return s.clone(),
            Value::Number(n) => return n.to_string(),
            _ => {}
        }
    }
    ulid::Ulid::new().to_string()
}
