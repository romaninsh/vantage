//! `CsvTableShell` — owns the typed `Table<Csv, EmptyEntity>` and exposes it
//! through the `TableShell` boundary.
//!
//! A CSV table is one file, read whole on every fetch. Equality conditions
//! become `Expression<AnyCsvType>`s on the wrapped table, which filters the
//! parsed rows; windows are cut from the filtered rows in memory. A clone
//! copies the table's conditions and shares nothing mutable. The backend is
//! read-only.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Column as VistaColumn, Condition, FilterOp, Page, Preview,
    Reference as VistaReference, TableShell, Vista, VistaCapabilities, VistaMetadata, unsupported,
};

use crate::csv::Csv;
use crate::operation::CsvOperation;
use crate::type_system::AnyCsvType;
use crate::vista::factory::CsvVistaFactory;

#[derive(Clone)]
pub struct CsvTableShell {
    pub(crate) table: Table<Csv, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    pub(crate) metadata: VistaMetadata,
}

impl CsvTableShell {
    pub(crate) fn new(
        table: Table<Csv, EmptyEntity>,
        capabilities: VistaCapabilities,
        metadata: VistaMetadata,
    ) -> Self {
        Self {
            table,
            capabilities,
            metadata,
        }
    }

    async fn read_all(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id, csv_record_to_cbor(record)))
            .collect())
    }
}

fn csv_record_to_cbor(record: Record<AnyCsvType>) -> Record<CborValue> {
    record.into_iter().map(|(k, v)| (k, v.into())).collect()
}

#[async_trait]
impl TableShell for CsvTableShell {
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
        "csv"
    }

    /// The file a read opens. Conditions never reach a server: they filter
    /// the parsed rows after the whole file is read.
    fn preview(&self) -> Preview {
        let file = self
            .table
            .data_source()
            .base_dir()
            .join(format!("{}.csv", self.table.table_name()));
        Preview::new("csv")
            .with_query(format!("read {}", file.display()))
            .with_note(
                "the whole file is read; conditions and windows are applied in memory afterwards",
            )
    }

    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        self.read_all().await
    }

    /// The file has no index, so the set is read and the row picked out.
    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        Ok(self.read_all().await?.shift_remove(id))
    }

    async fn count(&self) -> Result<i64> {
        Ok(self.read_all().await?.len() as i64)
    }

    /// The window is cut from the filtered rows, which are all read anyway,
    /// so the page carries the set's total for free.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let rows = self.read_all().await?;
        let total = rows.len() as i64;
        let window = rows.into_iter().skip(offset).take(limit).collect();
        Ok(Page::new(window).with_total(total))
    }

    /// Equality on a declared column. CSV evaluates no other operator.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for condition", field = field))?
            .clone();
        if condition.op != FilterOp::Eq {
            return unsupported(Capability::FilterOps);
        }
        let value: AnyCsvType = condition.value.clone().into();
        self.table.add_condition(column.eq(value));
        Ok(())
    }

    /// The target comes from the typed table's `with_one`/`with_many`; from a
    /// row it is narrowed by the reference's join keys.
    fn get_ref(&self, relation: &str, row: Option<&Record<CborValue>>) -> Result<Vista> {
        let reference = self
            .metadata
            .references
            .get(relation)
            .ok_or_else(|| error!("Unknown relation", relation = relation).mark_not_found())?;
        let target = self.table.get_ref_target_erased(relation)?;
        let target = CsvVistaFactory::new(self.table.data_source().clone()).from_table(target)?;
        match row {
            Some(row) => reference.narrow(target, row, CborValue::clone),
            None => Ok(target),
        }
    }
}
