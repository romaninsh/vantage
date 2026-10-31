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
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{
    Capability, Condition, FilterOp, Page, Preview, TableShell, Transaction, Vista,
    VistaCapabilities, VistaMetadata, condition_column_error, direct, unknown_relation,
    unsupported,
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
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "csv"
    }

    /// The file a read opens and the conditions that filter its parsed rows.
    fn preview(&self) -> Preview {
        let file = self
            .table
            .data_source()
            .base_dir()
            .join(format!("{}.csv", self.table.table_name()));
        let mut query = format!("read {}", file.display());
        let conditions: Vec<String> = self.table.conditions().map(|c| c.preview()).collect();
        if !conditions.is_empty() {
            query.push_str(" where ");
            query.push_str(&conditions.join(" and "));
        }
        Preview::new().with_query(query)
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
        Ok(Page::from_listing(self.read_all().await?, offset, limit))
    }

    /// Equality on a declared column. CSV evaluates no other operator, and
    /// refuses a dotted path into a declared column for the caller to
    /// evaluate.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let field = condition.column.as_str();
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| condition_column_error(&self.metadata, field))?
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
            .ok_or_else(|| unknown_relation(relation))?;
        self.table.traverse_erased(reference, row, |target| {
            CsvVistaFactory::new(self.table.data_source().clone()).from_table(target)
        })
    }

    /// A file has no transactions: inside one, the shell reads as it does
    /// outside.
    fn rebind(&self, _tx: &Transaction) -> Result<Box<dyn TableShell>> {
        Ok(direct(Box::new(self.clone())))
    }
}
