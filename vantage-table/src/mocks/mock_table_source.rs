use async_trait::async_trait;
use indexmap::IndexMap;
use rust_decimal::Decimal;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use vantage_dataset::InsertableValueSet;
use vantage_dataset::{
    ReadableValueSet, WritableValueSet,
    im::{ImDataSource, ImTable},
    traits::Result,
};
use vantage_expressions::{
    Expression, expr_any,
    mocks::datasource::MockSelectableDataSource,
    mocks::select::MockSelect,
    traits::datasource::{DataSource, ExprDataSource, SelectableDataSource},
    traits::expressive::{DeferredFn, ExpressiveEnum},
};
use vantage_types::{Entity, Record};

use crate::column::core::ColumnType;
use crate::mocks::mock_column::MockColumn;
use crate::mocks::mock_type_system::AnyMockType;
use crate::traits::table_expr_source::TableExprSource;
use crate::{
    table::Table,
    traits::{column_like::ColumnLike, table_source::TableSource},
};

/// A deliberately strict, in-memory `TableSource` for tests.
///
/// All row storage lives in a single [`ImDataSource`] — there is no second
/// count store to drift out of sync, so `get_table_count` always agrees with
/// what `list`/`get` return, including after writes.
///
/// Unlike the production [`ImTable`] (which honours the idempotent trait
/// contracts), this mock is intentionally *fail-loud* so tests can exercise
/// error paths: `insert_table_value` errors on a duplicate id, and
/// `delete_table_value` errors on a missing id. Treat divergence from the
/// documented idempotency as a feature of this test tool, not a bug.
#[derive(Clone)]
pub struct MockTableSource {
    im_data_source: ImDataSource,
    select_source: Option<MockSelectableDataSource>,
    query_source: Option<Arc<Mutex<vantage_expressions::mocks::mock_builder::MockBuilder>>>,
}

impl MockTableSource {
    pub fn new() -> Self {
        Self {
            im_data_source: ImDataSource::new(),
            select_source: None,
            query_source: None,
        }
    }

    pub async fn with_data(self, table_name: &str, data: Vec<Value>) -> Self {
        // Single store: seed rows straight into the ImDataSource. Every row must
        // carry a scalar `id` — fail loudly on test setup that doesn't, rather
        // than silently dropping rows (which used to make count and list
        // disagree).
        let im_table = ImTable::<vantage_types::EmptyEntity>::new(&self.im_data_source, table_name);
        for value in data.iter() {
            let id_str = match value.get("id") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                Some(other) => panic!(
                    "MockTableSource::with_data: row in '{table_name}' has a non-scalar `id` ({other:?}); ids must be a string or number"
                ),
                None => panic!(
                    "MockTableSource::with_data: row in '{table_name}' has no `id` field; every seed row must carry a scalar id"
                ),
            };
            let record = Record::from(value.clone());
            im_table
                .replace_value(&id_str, &record)
                .await
                .expect("Unable to replace value in im_table");
        }

        self
    }

    pub fn with_select_source(mut self, select_source: MockSelectableDataSource) -> Self {
        self.select_source = Some(select_source);
        self
    }

    pub fn with_query_source(
        mut self,
        query_source: vantage_expressions::mocks::mock_builder::MockBuilder,
    ) -> Self {
        self.query_source = Some(Arc::new(Mutex::new(query_source)));
        self
    }
}

impl Default for MockTableSource {
    fn default() -> Self {
        Self::new()
    }
}

impl DataSource for MockTableSource {}

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
            && type_id != TypeId::of::<crate::mocks::mock_type_system::AnyMockType>()
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
        parameters: Vec<vantage_expressions::traits::expressive::ExpressiveEnum<Self::Value>>,
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
        let pattern = format!("%{}%", search_value);
        expr_any!("name LIKE {}", pattern)
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
        im_table.list_values().await
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
        im_table.get_value(id).await
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
        Ok(self.im_data_source.table_len(table.table_name()) as i64)
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

    /// Insert a record as Record value (for WritableValueSet implementation)
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
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());

        // Check if record already exists - fail if it does
        if im_table.get_value(id).await?.is_some() {
            return Err(vantage_core::error!(
                "Record with ID already exists",
                id = id
            ));
        }

        let mut record_with_id = record.clone();
        record_with_id.insert("id".to_string(), Value::String(id.clone()));

        im_table.replace_value(id, &record_with_id).await
    }

    /// Replace a record as Record value (for WritableValueSet implementation)
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
        let mut record_with_id = record.clone();
        record_with_id.insert("id".to_string(), Value::String(id.clone()));

        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.replace_value(id, &record_with_id).await
    }

    /// Patch a record as Record value (for WritableValueSet implementation)
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
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.patch_value(id, partial).await
    }

    /// Delete a record by ID (for WritableValueSet implementation)
    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());

        // Check if record exists - fail if it doesn't
        if im_table.get_value(id).await?.is_none() {
            return Err(vantage_core::error!("Record not found", id = id));
        }

        im_table.delete(id).await
    }

    /// Delete all records (for WritableValueSet implementation)
    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.delete_all().await
    }

    /// Insert a record and return generated ID (for InsertableValueSet implementation)
    async fn insert_table_return_id_value<E>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity,
        Self: Sized,
    {
        let im_table = ImTable::<E>::new(&self.im_data_source, table.table_name());
        im_table.insert_return_id_value(record).await
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
        use vantage_expressions::{Expressive, Selectable, SelectableDataSource};

        // Build a subquery for source column values
        let mut select = self.select();
        select.add_source(source_table.table_name(), None);
        select.clear_fields();
        select.add_field(source_column);
        for condition in source_table.conditions() {
            select.add_where_condition(condition.clone());
        }

        // target_field IN (subquery)
        let tgt_col: Expression<Value> = Expression::new(target_field, vec![]);
        Expression::new(
            "{} IN ({})",
            vec![
                ExpressiveEnum::Nested(tgt_col),
                ExpressiveEnum::Nested(select.expr()),
            ],
        )
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> vantage_expressions::traits::associated_expressions::AssociatedExpression<
        'a,
        Self,
        Self::Value,
        Vec<Type>,
    >
    where
        E: Entity<Self::Value> + 'static,
        Self: Sized,
    {
        // MockTableSource uses select-based subquery (like SQL/SurrealDB)
        use crate::traits::column_like::ColumnLike;
        use vantage_expressions::traits::associated_expressions::AssociatedExpression;
        use vantage_expressions::{Expressive, Selectable, SelectableDataSource};
        let mut select = self.select();
        select.add_source(table.table_name(), None);
        select.clear_fields();
        select.add_field(column.name());
        for condition in table.conditions() {
            select.add_where_condition(condition.clone());
        }
        AssociatedExpression::new(select.expr(), self)
    }
}

impl ExprDataSource<Value> for MockTableSource {
    async fn execute(&self, expr: &Expression<Value>) -> vantage_core::Result<Value> {
        if let Some(ref query_source) = self.query_source {
            let source = {
                let guard = query_source.lock().unwrap();
                guard.clone()
            };
            source.execute(expr).await
        } else {
            panic!("MockTableSource query source not set. Use with_query_source() to configure it.")
        }
    }

    fn defer(
        &self,
        expr: Expression<Value>,
    ) -> vantage_expressions::traits::expressive::DeferredFn<Value>
    where
        Value: Clone + Send + Sync + 'static,
    {
        if let Some(ref query_source) = self.query_source {
            let query_source_clone = query_source.clone();
            let expr_clone = expr.clone();
            DeferredFn::new(move || {
                let query_source = query_source_clone.clone();
                let expr = expr_clone.clone();
                Box::pin(async move {
                    let source = {
                        let guard = query_source.lock().unwrap();
                        guard.clone()
                    };
                    match source.execute(&expr).await {
                        Ok(value) => Ok(ExpressiveEnum::Scalar(value)),
                        Err(e) => Err(e),
                    }
                })
            })
        } else {
            panic!("MockTableSource query source not set. Use with_query_source() to configure it.")
        }
    }
}

impl SelectableDataSource<Value> for MockTableSource {
    type Select = MockSelect;

    fn select(&self) -> Self::Select {
        if let Some(ref select_source) = self.select_source {
            select_source.select()
        } else {
            panic!(
                "MockTableSource select source not set. Use with_select_source() to configure it."
            )
        }
    }

    async fn execute_select(&self, select: &Self::Select) -> vantage_core::Result<Vec<Value>> {
        if let Some(ref select_source) = self.select_source {
            select_source.execute_select(select).await
        } else {
            panic!(
                "MockTableSource select source not set. Use with_select_source() to configure it."
            )
        }
    }
}

impl TableExprSource for MockTableSource {
    fn get_table_count_expr<E: Entity<Self::Value>>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_expressions::AssociatedExpression<'_, Self, Self::Value, usize> {
        let table_name = table.table_name();

        // Read the count synchronously from the single store — no block_on, no
        // separate count cache to go stale.
        let count = self.im_data_source.table_len(table_name);

        let expr = expr_any!("select count() from {}", table_name);

        // Seed the query source so executing `expr` resolves to this count.
        // The key must be the expression's rendered form (what the mock matches
        // on at execute time), not a hand-built string.
        if let Some(ref query_source) = self.query_source {
            let mut source = query_source.lock().unwrap();
            *source = source
                .clone()
                .on_exact_select(expr.preview(), serde_json::json!(count));
        }

        vantage_expressions::AssociatedExpression::new(expr, self)
    }

    fn get_table_sum_expr<E, R>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> vantage_expressions::AssociatedExpression<'_, Self, Self::Value, R>
    where
        E: Entity<Self::Value>,
        R: ColumnType + Default + std::ops::AddAssign,
    {
        let column_name = column.name();
        let table_name = table.table_name();
        let expr = expr_any!("select sum({}) from {}", column_name, table_name);
        vantage_expressions::AssociatedExpression::new(expr, self)
    }

    fn get_table_max_expr<E: Entity<Self::Value>, R: ColumnType>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> vantage_expressions::AssociatedExpression<'_, Self, Self::Value, R> {
        let column_name = column.name();
        let table_name = table.table_name();
        let expr = expr_any!("select max({}) from {}", column_name, table_name);
        vantage_expressions::AssociatedExpression::new(expr, self)
    }

    fn get_table_min_expr<E: Entity<Self::Value>, R: ColumnType>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> vantage_expressions::AssociatedExpression<'_, Self, Self::Value, R> {
        let column_name = column.name();
        let table_name = table.table_name();
        let expr = expr_any!("select min({}) from {}", column_name, table_name);
        vantage_expressions::AssociatedExpression::new(expr, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(serde::Deserialize, serde::Serialize, Debug, PartialEq, Default, Clone)]
    struct TestUser {
        id: i32,
        name: String,
    }

    #[tokio::test]
    async fn test_mock_table_source_with_data() {
        let mock = MockTableSource::new()
            .with_data(
                "users",
                vec![
                    json!({"id": "1", "name": "Alice"}),
                    json!({"id": "2", "name": "Bob"}),
                ],
            )
            .await;

        let table =
            Table::<MockTableSource, TestUser>::new("users", mock).into_entity::<TestUser>();
        let count = table.data_source().get_table_count(&table).await.unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_count_reflects_writes_and_count_expr_no_block_on() {
        use vantage_expressions::traits::datasource::ExprDataSource;

        let mock = MockTableSource::new()
            .with_data(
                "users",
                vec![
                    json!({"id": "1", "name": "Alice"}),
                    json!({"id": "2", "name": "Bob"}),
                ],
            )
            .await
            .with_query_source(vantage_expressions::mocks::mock_builder::new());
        let table = Table::<MockTableSource, TestUser>::new("users", mock);

        // Insert a third row; count derives from the single store, so it must
        // reflect the write (the old side-store left count stuck at 2).
        let mut rec = Record::new();
        rec.insert("name".to_string(), json!("Carol"));
        table
            .data_source()
            .insert_table_value(&table, &"3".to_string(), &rec)
            .await
            .unwrap();
        assert_eq!(
            table.data_source().get_table_count(&table).await.unwrap(),
            3
        );

        // get_expr_count() used to call Handle::block_on from inside the runtime
        // and panic. It now reads synchronously and seeds the query source, so
        // building and executing it under a runtime returns the live count.
        let count_expr = table.get_expr_count();
        let raw = table
            .data_source()
            .execute(count_expr.expression())
            .await
            .unwrap();
        assert_eq!(raw, json!(3));
    }

    #[tokio::test]
    #[should_panic(expected = "has no `id` field")]
    async fn test_with_data_panics_on_idless_row() {
        // Seed setup that drops the id used to silently vanish from reads while
        // still being counted; now it fails loudly.
        let _ = MockTableSource::new()
            .with_data("users", vec![json!({"name": "NoId"})])
            .await;
    }
}
