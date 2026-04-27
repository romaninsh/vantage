//! `TableSource` impl for `AwsAccount` (JSON-1.1 protocol).
//!
//! Read-only in v0. Writes are stubbed to error. Aggregations
//! (sum/min/max) likewise. The two interesting methods are
//! `list_table_values` (folds conditions into a JSON request body and
//! parses the response) and `column_table_values_expr` (returns a
//! deferred expression over the column's values — same shape as
//! `vantage-csv`). `related_in_condition` builds on top of that to
//! make `with_one` / `with_many` work for cross-resource navigation.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::json;

use vantage_core::error;
use vantage_dataset::traits::Result as DatasetResult;
use vantage_expressions::{
    Expression, Expressive, expr_any,
    traits::associated_expressions::AssociatedExpression,
    traits::datasource::ExprDataSource,
    traits::expressive::{DeferredFn, ExpressiveEnum},
};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::account::AwsAccount;
use crate::condition::AwsCondition;

#[async_trait]
impl TableSource for AwsAccount {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = CborValue;
    type Value = CborValue;
    type Id = String;
    type Condition = AwsCondition;

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
    ) -> Self::Condition
    where
        E: Entity<Self::Value>,
    {
        // No notion of which field is "searchable" at this layer.
        // Models that want full-text search add their own Eq on the
        // service-specific field (e.g. CloudWatch's filterPattern).
        AwsCondition::eq("__search__", json!(search_value).to_string())
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> DatasetResult<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let id_field = table.id_field().map(|c| c.name().to_string());
        let conditions: Vec<AwsCondition> = table.conditions().cloned().collect();
        let resp = self.execute_rpc(table.table_name(), &conditions).await?;
        Ok(self.parse_records(table.table_name(), resp, id_field.as_deref())?)
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> DatasetResult<Option<Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        // No native point-get for most JSON-1.1 APIs — list with the
        // table's conditions and pluck. Same honest cost as Mongo's
        // get-by-id without an index.
        let mut all = self.list_table_values(table).await?;
        Ok(all.shift_remove(id))
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> DatasetResult<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let all = self.list_table_values(table).await?;
        Ok(all.into_iter().next())
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> DatasetResult<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let all = self.list_table_values(table).await?;
        Ok(all.len() as i64)
    }

    async fn get_table_sum<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> DatasetResult<Self::Value>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("Aggregations not supported by vantage-aws"))
    }

    async fn get_table_max<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> DatasetResult<Self::Value>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("Aggregations not supported by vantage-aws"))
    }

    async fn get_table_min<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> DatasetResult<Self::Value>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("Aggregations not supported by vantage-aws"))
    }

    async fn insert_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> DatasetResult<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("vantage-aws is read-only in v0"))
    }

    async fn replace_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> DatasetResult<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("vantage-aws is read-only in v0"))
    }

    async fn patch_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _partial: &Record<Self::Value>,
    ) -> DatasetResult<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("vantage-aws is read-only in v0"))
    }

    async fn delete_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
    ) -> DatasetResult<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("vantage-aws is read-only in v0"))
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> DatasetResult<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("vantage-aws is read-only in v0"))
    }

    async fn insert_table_return_id_value<E>(
        &self,
        _table: &Table<Self, E>,
        _record: &Record<Self::Value>,
    ) -> DatasetResult<Self::Id>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("vantage-aws is read-only in v0"))
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
        // Build "target_field IN (subquery)" as a Deferred condition:
        // at execute time, the embedded expression runs the source
        // query, projects `source_column`, and we apply the same
        // take-1-or-error rule as any other Deferred. AWS doesn't
        // accept multi-value filters, so traversal is implicitly
        // single-parent — multi-row sources error loudly.
        let src_col = self.create_column::<Self::AnyType>(source_column);
        let values_expr = self.column_table_values_expr(source_table, &src_col);
        AwsCondition::Deferred {
            field: target_field.to_string(),
            source: values_expr.expr(),
        }
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: Sized,
    {
        // Same shape as `vantage-csv`'s impl: wrap a `DeferredFn` that
        // runs `list_table_values` and projects the named column at
        // execute time. Caller goes through `AssociatedExpression::get`
        // for direct execution, or `.expr()` to embed in a bigger
        // expression (e.g. a deferred condition on another table).
        let table_clone = table.clone();
        let col = column.name().to_string();
        let aws = self.clone();

        let inner = expr_any!("{}", {
            DeferredFn::new(move || {
                let aws = aws.clone();
                let table = table_clone.clone();
                let col = col.clone();
                Box::pin(async move {
                    let records = aws.list_table_values(&table).await?;
                    let values: Vec<CborValue> = records
                        .values()
                        .filter_map(|r| r.get(&col).cloned())
                        .collect();
                    Ok(ExpressiveEnum::Scalar(CborValue::Array(values)))
                })
            })
        });

        let expr = expr_any!("{}", { self.defer(inner) });
        AssociatedExpression::new(expr, self)
    }
}
