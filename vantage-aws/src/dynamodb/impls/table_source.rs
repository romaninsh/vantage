//! `TableSource` for DynamoDB.
//!
//! v0 wires the read/write path through `Scan`, `GetItem`, `PutItem`,
//! `DeleteItem`. Conditions and aggregates are stubbed — Scan filters
//! and SUM/MIN/MAX-via-client-aggregation land later.
//!
//! Composite-key tables are partially supported: writes carry the full
//! item, but `get_table_value` only knows the partition key (`DynamoId`
//! is partition-only in v0). Sort-key tables work for Scan/Put/Delete
//! when the caller hands in items containing both keys.

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
use crate::dynamodb::condition::DynamoCondition;
use crate::dynamodb::id::DynamoId;
use crate::dynamodb::transport;
use crate::dynamodb::types::{AnyDynamoType, AttributeValue};
use crate::dynamodb::wire::{attr_to_json, json_to_item_map};

const DEFAULT_ID_FIELD: &str = "id";

fn id_field_name<E: Entity<AnyDynamoType>>(table: &Table<DynamoDB, E>) -> String {
    table
        .id_field()
        .map(|c| c.name().to_string())
        .unwrap_or_else(|| DEFAULT_ID_FIELD.to_string())
}

/// Build a single-field DynamoDB `Key` map from a partition-key id.
fn key_for_id(field: &str, id: &DynamoId) -> std::result::Result<JsonMap<String, JsonValue>, vantage_core::VantageError> {
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
        let resp = transport::scan(self.aws(), table.table_name(), None, false).await?;
        let items = resp
            .get("Items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| error!("DynamoDB Scan response missing Items array"))?;

        let id_field = id_field_name(table);
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
        let id_field = id_field_name(table);
        let key = key_for_id(&id_field, id)?;
        let resp = transport::get_item(self.aws(), table.table_name(), key).await?;

        let Some(item) = resp.get("Item") else {
            return Ok(None);
        };
        if item.is_null() {
            return Ok(None);
        }
        let (_id, record) = item_to_record(&id_field, item)?;
        Ok(Some(record))
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
    {
        let resp = transport::scan(self.aws(), table.table_name(), Some(1), false).await?;
        let items = resp
            .get("Items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| error!("DynamoDB Scan response missing Items array"))?;
        let Some(item) = items.first() else {
            return Ok(None);
        };
        let id_field = id_field_name(table);
        let (id, record) = item_to_record(&id_field, item)?;
        Ok(Some((id, record)))
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
    {
        let resp = transport::scan(self.aws(), table.table_name(), None, true).await?;
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
        let id_field = id_field_name(table);
        let mut item = JsonMap::new();
        item.insert(id_field.clone(), attr_to_json(&id.to_attr())?);
        for (k, v) in record.iter() {
            if k == &id_field {
                continue;
            }
            item.insert(k.clone(), attr_to_json(v.value())?);
        }
        transport::put_item(self.aws(), table.table_name(), item).await?;

        // PutItem doesn't return the written item by default; re-fetch
        // so callers see exactly what's now in storage (including any
        // columns DynamoDB may have stored differently).
        self.get_table_value(table, id).await?.ok_or_else(|| {
            error!("Inserted item not found by GetItem", id = id.to_string())
        })
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
        // PutItem replaces by default — same code path as insert.
        self.insert_table_value(table, id, record).await
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
        // UpdateItem with SET expression — needs placeholder mangling
        // to avoid colliding with reserved words and is non-trivial to
        // render. Land it next iteration.
        Err(error!(
            "DynamoDB UpdateItem (patch) not implemented yet — use replace"
        ))
    }

    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let id_field = id_field_name(table);
        let key = key_for_id(&id_field, id)?;
        transport::delete_item(self.aws(), table.table_name(), key).await?;
        Ok(())
    }

    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
    {
        let id_field = id_field_name(table);
        let resp = transport::scan(self.aws(), table.table_name(), None, false).await?;
        let items = resp
            .get("Items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| error!("DynamoDB Scan response missing Items array"))?;

        for item in items {
            let pairs = json_to_item_map(item)?;
            let id_av = pairs.iter().find(|(k, _)| k == &id_field).map(|(_, v)| v.clone());
            if let Some(av) = id_av
                && let Some(id) = DynamoId::from_attr(&av)
            {
                let key = key_for_id(&id_field, &id)?;
                transport::delete_item(self.aws(), table.table_name(), key).await?;
            }
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
        _target_field: &str,
        _source_table: &Table<Self, SourceE>,
        _source_column: &str,
    ) -> Self::Condition
    where
        Self: Sized,
    {
        // Stub: no relationship traversal in v0. Returning a benign
        // placeholder lets compile-time wiring exist; runtime use is
        // gated by callers not constructing relationships yet.
        DynamoCondition::eq("__related__", AttributeValue::Null)
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

