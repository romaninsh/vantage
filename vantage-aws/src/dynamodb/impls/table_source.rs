//! `TableSource` for DynamoDB.
//!
//! v0 wires the read/write path through `Scan`, `GetItem`, `PutItem`,
//! `DeleteItem`. Conditions and aggregates are stubbed — Scan filters
//! and SUM/MIN/MAX-via-client-aggregation land later.
//!
//! Insert is a conditional `PutItem` (`attribute_not_exists`), so an id held
//! outside the set is detected (strongly consistent per item) and never
//! overwritten. A set whose conditions DynamoDB can't evaluate client-side
//! refuses the write.
//!
//! Composite-key tables are partially supported: writes carry the full
//! item, but `get_table_value` only knows the partition key (`DynamoId`
//! is partition-only in v0). Sort-key tables work for Scan/Put/Delete
//! when the caller hands in items containing both keys.

use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::{Map as JsonMap, Value as JsonValue};

use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::{
    Expression, expr_any,
    traits::associated_expressions::AssociatedExpression,
    traits::datasource::ExprDataSource,
    traits::expressive::{DeferredFn, ExpressiveEnum},
};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::dynamodb::DynamoDB;
use crate::dynamodb::condition::{
    DynamoCondition, ResolvedFilter, ValueListFuture, equality_of, resolve_conditions, row_in_set,
};
use crate::dynamodb::id::DynamoId;
use crate::dynamodb::transport::{self, PutOutcome};
use crate::dynamodb::types::{AnyDynamoType, AttributeValue};
use crate::dynamodb::wire::{attr_to_json, json_to_item_map};

/// Build a single-field DynamoDB `Key` map from a partition-key id.
fn key_for_id(
    field: &str,
    id: &DynamoId,
) -> std::result::Result<JsonMap<String, JsonValue>, vantage_core::VantageError> {
    let mut key = JsonMap::new();
    key.insert(field.to_string(), attr_to_json(&id.to_attr())?);
    Ok(key)
}

/// Convert a wire item (from a Scan/GetItem response) into our Record
/// shape and pull the configured id field out as a `DynamoId`.
fn item_to_record(
    id_field: &str,
    item: &JsonValue,
) -> std::result::Result<(DynamoId, Record<AnyDynamoType>), vantage_core::VantageError> {
    let pairs = json_to_item_map(item)?;
    let mut id: Option<DynamoId> = None;
    let mut record = Record::new();
    for (k, av) in pairs {
        if k == id_field {
            id = DynamoId::from_attr(&av);
        }
        record.insert(k, AnyDynamoType::untyped(av));
    }
    let id = id.ok_or_else(|| {
        error!(
            "DynamoDB item missing id field",
            id_field = id_field.to_string()
        )
    })?;
    Ok((id, record))
}

/// Build the wire item for a write: the id plus every non-id column.
fn item_map(
    id_field: &str,
    id: &DynamoId,
    record: &Record<AnyDynamoType>,
) -> std::result::Result<JsonMap<String, JsonValue>, vantage_core::VantageError> {
    let mut item = JsonMap::new();
    item.insert(id_field.to_string(), attr_to_json(&id.to_attr())?);
    for (k, v) in record.iter() {
        if k == id_field {
            continue;
        }
        item.insert(k.clone(), attr_to_json(v.value())?);
    }
    Ok(item)
}

/// Refuse a write whose row (record plus id) falls outside the set.
async fn ensure_in_set(
    conditions: &[DynamoCondition],
    id_field: &str,
    id: &DynamoId,
    record: &Record<AnyDynamoType>,
) -> Result<()> {
    let mut row = record.clone();
    row.insert(id_field.to_string(), AnyDynamoType::untyped(id.to_attr()));
    if row_in_set(conditions, &row).await? {
        Ok(())
    } else {
        Err(error!("Row does not belong to this set", id = id.to_string()).mark_conflict())
    }
}

#[async_trait]
impl TableSource for DynamoDB {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnyDynamoType;
    type Value = AnyDynamoType;
    type Id = DynamoId;
    type Condition = DynamoCondition;
    type Source = String;

    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type> {
        Column::new(name)
    }

    fn eq_condition(field: &str, value: &str) -> vantage_core::Result<Self::Condition> {
        Ok(DynamoCondition::eq(
            field,
            AttributeValue::S(value.to_string()),
        ))
    }

    fn condition_equality(&self, condition: &Self::Condition) -> Option<(String, Self::Value)> {
        equality_of(condition).map(|(field, value)| (field, AnyDynamoType::untyped(value)))
    }

    fn can_confine_writes(&self) -> bool {
        true
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
        // No notion of which fields are searchable at this layer.
        // Stash the value in an Eq against `__search__` so callers see
        // a deterministic shape; real FilterExpression search is v1.
        DynamoCondition::eq("__search__", AttributeValue::S(search_value.to_string()))
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        let filter = resolve_conditions(table.conditions()).await?;
        let resp =
            transport::scan(self.aws(), table.table_name(), None, false, Some(&filter)).await?;
        let items = resp
            .get("Items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| error!("DynamoDB Scan response missing Items array"))?;

        let id_field = table.id_field_name();
        let mut out = IndexMap::with_capacity(items.len());
        for item in items {
            let (id, record) = item_to_record(&id_field, item)?;
            out.insert(id, record);
        }
        Ok(out)
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Option<Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
    {
        let id_field = table.id_field_name();
        let key = key_for_id(&id_field, id)?;
        let resp = transport::get_item(self.aws(), table.table_name(), key).await?;

        let Some(item) = resp.get("Item") else {
            return Ok(None);
        };
        if item.is_null() {
            return Ok(None);
        }
        let (_id, record) = item_to_record(&id_field, item)?;
        let conditions: Vec<DynamoCondition> = table.conditions().cloned().collect();
        Ok(row_in_set(&conditions, &record).await?.then_some(record))
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
    {
        let filter = resolve_conditions(table.conditions()).await?;
        // DynamoDB applies FilterExpression *after* Limit, so a naive
        // Scan(Limit=1) with a filter routinely returns zero even when
        // matches exist. Page through with a moderate per-page chunk and
        // break on the first match — bounds memory and round-trips while
        // still walking the table when the filter is selective. Without a
        // filter, page_limit=1 short-circuits after a single round-trip.
        let page_limit = if filter.is_empty() { 1 } else { 100 };
        let item =
            transport::scan_first_match(self.aws(), table.table_name(), page_limit, Some(&filter))
                .await?;
        let Some(item) = item else {
            return Ok(None);
        };
        let id_field = table.id_field_name();
        let (id, record) = item_to_record(&id_field, &item)?;
        Ok(Some((id, record)))
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        let filter = resolve_conditions(table.conditions()).await?;
        let resp =
            transport::scan(self.aws(), table.table_name(), None, true, Some(&filter)).await?;
        let count = resp
            .get("Count")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| error!("DynamoDB Scan(COUNT) response missing Count"))?;
        Ok(count)
    }

    async fn get_table_sum<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        Err(error!(
            "DynamoDB has no native SUM — caller must aggregate client-side"
        ))
    }

    async fn get_table_max<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        Err(error!(
            "DynamoDB has no native MAX — caller must aggregate client-side"
        ))
    }

    async fn get_table_min<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        Err(error!(
            "DynamoDB has no native MIN — caller must aggregate client-side"
        ))
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
        let id_field = table.id_field_name();
        let conditions: Vec<DynamoCondition> = table.conditions().cloned().collect();
        let item = item_map(&id_field, id, record)?;
        ensure_in_set(&conditions, &id_field, id, record).await?;

        let not_exists = ResolvedFilter {
            expression: "attribute_not_exists(#pk)".to_string(),
            names: IndexMap::from([("#pk".to_string(), id_field.clone())]),
            values: IndexMap::new(),
        };
        let outcome =
            transport::put_item_if(self.aws(), table.table_name(), item, &not_exists).await?;

        // An id that is already stored is never overwritten: inside the set
        // the stored item is returned, outside it the insert conflicts.
        // PutItem doesn't return the written item, so re-fetch it.
        match (outcome, self.get_table_value(table, id).await?) {
            (_, Some(stored)) => Ok(stored),
            (PutOutcome::Written, None) => Err(error!(
                "Inserted item not found by GetItem",
                id = id.to_string()
            )),
            (PutOutcome::ConditionFailed, None) => {
                Err(error!("Item exists outside this set", id = id.to_string()).mark_conflict())
            }
        }
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
        let id_field = table.id_field_name();
        let conditions: Vec<DynamoCondition> = table.conditions().cloned().collect();
        let item = item_map(&id_field, id, record)?;
        ensure_in_set(&conditions, &id_field, id, record).await?;

        // Writes when the id is free or the stored item is inside the set.
        let filter = resolve_conditions(&conditions).await?;
        let mut guard = filter.clone();
        if !filter.is_empty() {
            guard.expression = format!("attribute_not_exists(#pk) OR ({})", filter.expression);
            guard.names.insert("#pk".to_string(), id_field.clone());
        }
        match transport::put_item_if(self.aws(), table.table_name(), item, &guard).await? {
            PutOutcome::Written => self
                .get_table_value(table, id)
                .await?
                .ok_or_else(|| error!("Replaced item not found by GetItem", id = id.to_string())),
            PutOutcome::ConditionFailed => {
                Err(error!("Item exists outside this set", id = id.to_string()).mark_conflict())
            }
        }
    }

    async fn patch_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
    {
        Err(error!("DynamoDB patch is not available; use replace").mark_unsupported())
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let id_field = table.id_field_name();
        let key = key_for_id(&id_field, id)?;
        let filter = resolve_conditions(table.conditions()).await?;
        // A delete the condition refuses means the item is outside the set
        // (or already gone): the retry-safe outcome is success.
        transport::delete_item_if(self.aws(), table.table_name(), key, &filter).await?;
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let id_field = table.id_field_name();
        let filter = resolve_conditions(table.conditions()).await?;
        let resp =
            transport::scan(self.aws(), table.table_name(), None, false, Some(&filter)).await?;
        let items = resp
            .get("Items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| error!("DynamoDB Scan response missing Items array"))?;

        for item in items {
            let pairs = json_to_item_map(item)?;
            let av = pairs
                .iter()
                .find(|(k, _)| k == &id_field)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| {
                    error!(
                        "DynamoDB scan item missing id field — refusing partial delete",
                        id_field = id_field.clone()
                    )
                })?;
            let id = DynamoId::from_attr(&av).ok_or_else(|| {
                error!(
                    "DynamoDB partition key has unsupported AttributeValue type — \
                     v0 DynamoId only handles S/N. Refusing partial delete.",
                    id_field = id_field.clone(),
                    got = format!("{:?}", av)
                )
            })?;
            let key = key_for_id(&id_field, &id)?;
            transport::delete_item(self.aws(), table.table_name(), key).await?;
        }
        Ok(())
    }

    async fn insert_table_return_id_value<E>(
        &self,
        _table: &Table<Self, E>,
        _record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
    {
        Err(error!(
            "DynamoDB does not auto-generate primary keys; use insert_table_value"
        ))
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
        let db = self.clone();
        let source = source_table.clone();
        let col = source_column.to_string();
        let field = target_field.to_string();

        let values: super::super::condition::ValueListFn = Arc::new(move || -> ValueListFuture {
            let db = db.clone();
            let source = source.clone();
            let col = col.clone();
            Box::pin(async move {
                let records = db.list_table_values(&source).await?;
                let values: Vec<AttributeValue> = records
                    .values()
                    .filter_map(|r| r.get(&col).map(|v| v.value().clone()))
                    .collect();
                Ok(values)
            })
        });

        DynamoCondition::In { field, values }
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
        // Mirror the AwsAccount/CSV shape: defer a Scan + project the
        // named column. Wraps execute_rpc-equivalent (Scan) at exec time.
        let table_clone = table.clone();
        let col = column.name().to_string();
        let db = self.clone();

        let inner = expr_any!("{}", {
            DeferredFn::new(move || {
                let db = db.clone();
                let table = table_clone.clone();
                let col = col.clone();
                Box::pin(async move {
                    let records = db.list_table_values(&table).await?;
                    let values: Vec<AnyDynamoType> = records
                        .values()
                        .filter_map(|r| r.get(&col).cloned())
                        .collect();
                    Ok(ExpressiveEnum::Scalar(AnyDynamoType::new(values)))
                })
            })
        });

        let expr = expr_any!("{}", { self.defer(inner) });
        AssociatedExpression::new(expr, self)
    }
}
