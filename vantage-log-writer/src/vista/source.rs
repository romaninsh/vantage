//! `LogWriterTableShell` — an append-only JSONL sink behind the `TableShell`
//! boundary.
//!
//! The shell only inserts. Nothing it writes can be read back, so every read
//! refuses with `unsupported(Capability::Window)`: the window is the read
//! family's one capability, and the shell advertises none of it. Equality
//! conditions on declared columns are recorded so a narrowed handle is
//! honest about its query, but no read ever runs, and `Vista` refuses writes
//! through a narrowed handle since the shell doesn't advertise
//! `ConfineWrites`.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::Value;
use vantage_core::{Result, error};
use vantage_dataset::traits::WritableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Column as VistaColumn, Condition, FilterOp, Page, Preview,
    Reference as VistaReference, TableShell, Vista, VistaCapabilities, VistaMetadata, unsupported,
};

use crate::log_writer::LogWriter;
use crate::table_source::extract_or_generate_id;

#[derive(Clone)]
pub struct LogWriterTableShell {
    pub(crate) table: Table<LogWriter, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
    /// Equality conditions applied to this handle. Reads refuse and narrowed
    /// writes are refused by `Vista`, so they never filter anything; they are
    /// kept so the preview reports them.
    pub(crate) conditions: Vec<Condition>,
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
            conditions: Vec::new(),
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

/// The refusal every read answers with: `Window` is the read family's
/// capability, and the context says why the shell lacks it.
fn refuse_read<T>() -> Result<T> {
    unsupported(Capability::Window).map_err(|mut e| {
        e.context
            .insert("reason".to_string(), format!("{:?}", "write-only sink"));
        e
    })
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

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "log-writer"
    }

    /// Write-only: there is no read query to preview.
    fn preview(&self) -> Preview {
        let mut note = String::from(
            "write-only sink: it accepts inserts and refuses every read, so no query is sent",
        );
        if !self.conditions.is_empty() {
            note.push_str("; conditions are recorded but never evaluated");
        }
        Preview::new("log-writer").with_note(note)
    }

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        refuse_read()
    }

    async fn get(&self, _id: &str) -> Result<Option<Record<CborValue>>> {
        refuse_read()
    }

    async fn count(&self) -> Result<i64> {
        refuse_read()
    }

    async fn fetch_window(&self, _offset: usize, _limit: usize) -> Result<Page> {
        refuse_read()
    }

    /// Equality on a declared column, recorded on this handle only.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        if !self.metadata.columns.contains_key(field) {
            return Err(error!("Unknown column for condition", field = field));
        }
        if condition.op != FilterOp::Eq {
            return unsupported(Capability::FilterOps);
        }
        self.conditions.push(condition.clone());
        Ok(())
    }

    /// Append one line. With no `id`, the record's own id column is used,
    /// or a fresh ULID. The returned row is the projected line as queued.
    async fn insert(
        &self,
        id: Option<&str>,
        record: &Record<CborValue>,
    ) -> Result<(String, Record<CborValue>)> {
        let json_record = cbor_record_to_json(record);
        let id = match id {
            Some(id) => id.to_string(),
            None => extract_or_generate_id(&json_record, self.table.data_source().id_column()),
        };
        let stored = self.table.insert_value(&id, &json_record).await?;
        let stored = stored
            .into_iter()
            .map(|(k, v)| (k, vantage_types::json_to_cbor(v)))
            .collect();
        Ok((id, stored))
    }

    fn get_ref(&self, _relation: &str, _row: Option<&Record<CborValue>>) -> Result<Vista> {
        unsupported(Capability::Traverse)
    }
}
