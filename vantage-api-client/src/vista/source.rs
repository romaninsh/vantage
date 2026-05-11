//! `RestApiTableShell` — owns the typed `Table<RestApi, EmptyEntity>` and
//! exposes it through the `TableShell` boundary.
//!
//! REST responses are JSON; vista's wire type is CBOR. Translation is a
//! serde round-trip: `ciborium::Value::serialized` one way,
//! `serde_json::to_value` the other. Same bytes that pass through
//! `serde_json::Value` round-trip cleanly; the only lossy edges are NaN
//! and binary-as-string, neither of which appears in vanilla REST traffic.
//!
//! `add_eq_condition` translates the CBOR value back to JSON and uses
//! `eq_condition` (already present in the crate) to push an eq condition
//! onto the wrapped table — `RestApi::fetch_records` then folds it into
//! the URL query string the same way it does for hand-built conditions.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{TableShell, Vista, VistaCapabilities};

use crate::RestApi;
use crate::eq_condition;

pub struct RestApiTableShell {
    pub(crate) table: Table<RestApi, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
}

impl RestApiTableShell {
    pub(crate) fn new(
        table: Table<RestApi, EmptyEntity>,
        capabilities: VistaCapabilities,
    ) -> Self {
        Self {
            table,
            capabilities,
        }
    }
}

fn json_to_cbor(v: JsonValue) -> CborValue {
    CborValue::serialized(&v).unwrap_or(CborValue::Null)
}

fn cbor_to_json(v: CborValue) -> JsonValue {
    serde_json::to_value(v).unwrap_or(JsonValue::Null)
}

fn json_record_to_cbor(record: Record<JsonValue>) -> Record<CborValue> {
    record.into_iter().map(|(k, v)| (k, json_to_cbor(v))).collect()
}

#[async_trait]
impl TableShell for RestApiTableShell {
    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id, json_record_to_cbor(record)))
            .collect())
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        Ok(self.table.get_value(id).await?.map(json_record_to_cbor))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        Ok(self
            .table
            .get_some_value()
            .await?
            .map(|(id, record)| (id, json_record_to_cbor(record))))
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Ok(self.table.list_values().await?.len() as i64)
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        if !self.table.columns().contains_key(field) {
            return Err(error!("Unknown column for eq condition", field = field));
        }
        let json = cbor_to_json(value.clone());
        self.table.add_condition(eq_condition(field, json));
        Ok(())
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "rest-api"
    }
}
