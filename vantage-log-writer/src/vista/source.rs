use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value;
use vantage_core::{Result, error};
use vantage_dataset::traits::InsertableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Column as VistaColumn, Reference as VistaReference, TableShell, Vista, VistaCapabilities,
    VistaMetadata,
};

use crate::log_writer::LogWriter;

pub struct LogWriterTableShell {
    pub(crate) table: Table<LogWriter, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
}

impl LogWriterTableShell {
    pub(crate) fn new(
        table: Table<LogWriter, EmptyEntity>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
        }
    }
}

fn cbor_record_to_json(record: &Record<CborValue>) -> Record<Value> {
    record
        .iter()
        .map(|(k, v)| {
            let json = vantage_types::cbor_to_json(&vantage_types::PlainDialect, v.clone());
            (k.clone(), json)
        })
        .collect()
}

#[async_trait]
impl TableShell for LogWriterTableShell {
    fn columns(&self) -> &IndexMap<String, VistaColumn> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, VistaReference> {
        &self.metadata.references
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        Err(self.default_error("list_vista_values", "can_count"))
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        _id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        Err(self.default_error("get_vista_value", "can_count"))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        Err(self.default_error("get_vista_some_value", "can_count"))
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Err(self.default_error("get_vista_count", "can_count"))
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
            .map(|(k, v)| (k, vantage_types::json_to_cbor(v)))
            .collect())
    }

    fn add_eq_condition(&mut self, _field: &str, _value: &CborValue) -> Result<()> {
        Err(
            error!("log-writer is insert-only; conditions are not supported")
                .mark_unsupported()
                .traced(),
        )
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "log-writer"
    }
}
