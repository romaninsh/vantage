//! `TableSource` impl for [`KubernetesCluster`].
//!
//! `list_table_values` does the real work: GET the resource list, run each
//! object through its projector (flatten + parse), then apply equality
//! conditions client-side. Because every join key (`namespace`,
//! `nodeName`, `ownerDeployment`, …) is a projected flat field, the same
//! post-fetch filter that AWS uses makes relations narrow correctly. v0 is
//! read-only; writes and aggregates error.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;

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

use crate::cluster::KubernetesCluster;
use crate::condition::{KubeCondition, eq_pairs};

impl KubernetesCluster {
    /// Materialise any `Deferred` condition into an `Eq` by running its
    /// embedded query. K8s relations narrow from a single parent, so the
    /// resolved value list must contain exactly one element.
    async fn resolve_conditions(
        &self,
        conditions: &[KubeCondition],
    ) -> vantage_core::Result<Vec<KubeCondition>> {
        let mut out = Vec::with_capacity(conditions.len());
        for cond in conditions {
            match cond {
                KubeCondition::Deferred { field, source } => {
                    let payload = ExprDataSource::execute(self, source).await?;
                    let values = match payload {
                        CborValue::Array(items) => items,
                        other => vec![other],
                    };
                    match values.len() {
                        1 => out.push(KubeCondition::Eq {
                            field: field.clone(),
                            value: values.into_iter().next().unwrap(),
                        }),
                        0 => {
                            return Err(error!(
                                "Deferred condition resolved to zero values",
                                field = field.as_str()
                            ));
                        }
                        n => {
                            return Err(error!(
                                "Kubernetes relations narrow from a single parent; \
                                 deferred condition resolved to many",
                                field = field.as_str(),
                                count = n
                            ));
                        }
                    }
                }
                other => out.push(other.clone()),
            }
        }
        Ok(out)
    }
}

#[async_trait]
impl TableSource for KubernetesCluster {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = CborValue;
    type Value = CborValue;
    type Id = String;
    type Condition = KubeCondition;
    type Source = String;

    fn eq_condition(field: &str, value: &str) -> DatasetResult<Self::Condition> {
        Ok(KubeCondition::eq(field.to_string(), value.to_string()))
    }

    fn eq_value_condition(&self, field: &str, value: Self::Value) -> DatasetResult<Self::Condition> {
        Ok(KubeCondition::eq(field.to_string(), value))
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
        _table: &Table<Self, E>,
        search_value: &str,
    ) -> Self::Condition
    where
        E: Entity<Self::Value>,
    {
        // No generic free-text index at this layer; a sentinel field keeps
        // the contract total without matching real rows.
        KubeCondition::eq("__search__", search_value.to_string())
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> DatasetResult<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let api_path = table.table_name();
        let conditions: Vec<KubeCondition> = table.conditions().cloned().collect();
        let resolved = self.resolve_conditions(&conditions).await?;

        let items = self.list_items(api_path).await?;
        let mut records: IndexMap<String, Record<CborValue>> = IndexMap::new();
        for item in &items {
            if let Some((id, record)) = crate::models::project_for(api_path, item) {
                records.insert(id, record);
            }
        }

        // Client-side equality narrowing. Unlike AWS (where an absent field
        // means "server-side request param, already applied"), every K8s
        // condition names a projected field, so a record missing that field
        // genuinely doesn't match — exclude it. This is what makes relation
        // drilling narrow instead of over-showing.
        let pairs = eq_pairs(&resolved)?;
        if !pairs.is_empty() {
            records.retain(|_id, record| {
                pairs
                    .iter()
                    .all(|(field, value)| record.get(field) == Some(value))
            });
        }

        Ok(records)
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
        Ok(self.list_table_values(table).await?.len() as i64)
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
        Err(error!("Aggregations not supported by vantage-kubernetes"))
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
        Err(error!("Aggregations not supported by vantage-kubernetes"))
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
        Err(error!("Aggregations not supported by vantage-kubernetes"))
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
        Err(error!("vantage-kubernetes is read-only in v0"))
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
        Err(error!("vantage-kubernetes is read-only in v0"))
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
        Err(error!("vantage-kubernetes is read-only in v0"))
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> DatasetResult<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("vantage-kubernetes is read-only in v0"))
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> DatasetResult<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("vantage-kubernetes is read-only in v0"))
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
        Err(error!("vantage-kubernetes is read-only in v0"))
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
        let values_expr = self.column_table_values_expr(source_table, &src_col);
        KubeCondition::Deferred {
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
        let table_clone = table.clone();
        let col = column.name().to_string();
        let cluster = self.clone();

        let inner = expr_any!("{}", {
            DeferredFn::new(move || {
                let cluster = cluster.clone();
                let table = table_clone.clone();
                let col = col.clone();
                Box::pin(async move {
                    let records = cluster.list_table_values(&table).await?;
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
