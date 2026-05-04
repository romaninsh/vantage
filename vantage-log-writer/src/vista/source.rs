use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value;
use vantage_core::{Result, error};
use vantage_dataset::traits::InsertableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{TableShell, Vista, VistaCapabilities};

use crate::log_writer::LogWriter;

pub struct LogWriterTableShell {
    pub(crate) table: Table<LogWriter, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
}

impl LogWriterTableShell {
    pub(crate) fn new(
        table: Table<LogWriter, EmptyEntity>,
        capabilities: VistaCapabilities,
    ) -> Self {
        Self {
            table,
            capabilities,
        }
    }
}

fn cbor_record_to_json(record: &Record<CborValue>) -> Record<Value> {
    record
        .iter()
        .map(|(k, v)| {
            let json = serde_json::to_value(v).unwrap_or(Value::Null);
            (k.clone(), json)
        })
        .collect()
}

#[async_trait]
impl TableShell for LogWriterTableShell {
    async fn list_vista_values(
        &self,
        vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        Err(self.default_error("list_vista_values", "can_count", vista))
    }

    async fn get_vista_value(
        &self,
        vista: &Vista,
        _id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        Err(self.default_error("get_vista_value", "can_count", vista))
    }

    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        Err(self.default_error("get_vista_some_value", "can_count", vista))
    }

    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        Err(self.default_error("get_vista_count", "can_count", vista))
    }

    async fn insert_vista_return_id_value(
        &self,
        _vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        let json_record = cbor_record_to_json(record);
        self.table.insert_return_id_value(&json_record).await
    }

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        use vantage_dataset::traits::WritableValueSet;
        let json_record = cbor_record_to_json(record);
        let stored = self.table.insert_value(id, &json_record).await?;
        Ok(stored
            .into_iter()
            .map(|(k, v)| {
                let cbor = ciborium::Value::serialized(&v).unwrap_or(ciborium::Value::Null);
                (k, cbor)
            })
            .collect())
    }

    fn add_eq_condition(&mut self, _field: &str, _value: &CborValue) -> Result<()> {
        Err(error!("log-writer is insert-only; conditions are not supported").is_unsupported())
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "log-writer"
    }
}
