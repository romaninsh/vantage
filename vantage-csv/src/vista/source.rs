//! `CsvTableShell` — owns the typed `Table<Csv, EmptyEntity>` and exposes it
//! through the `TableShell` boundary.
//!
//! `add_eq_condition` translates `(field, CborValue)` into an
//! `Expression<AnyCsvType>` and pushes it onto the wrapped table; the table's
//! existing condition machinery does the actual filtering on the next read.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{Vista, VistaCapabilities, TableShell};

use crate::csv::Csv;
use crate::operation::CsvOperation;
use crate::type_system::AnyCsvType;

pub struct CsvTableShell {
    pub(crate) table: Table<Csv, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
}

impl CsvTableShell {
    pub(crate) fn new(table: Table<Csv, EmptyEntity>, capabilities: VistaCapabilities) -> Self {
        Self {
            table,
            capabilities,
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
    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.read_all().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let mut data = self.read_all().await?;
        Ok(data.shift_remove(id))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let data = self.read_all().await?;
        Ok(data.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Ok(self.read_all().await?.len() as i64)
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let column = self
            .table
            .columns()
            .get(field)
            .ok_or_else(|| error!("Unknown column for eq condition", field = field))?
            .clone();
        let csv_value: AnyCsvType = value.clone().into();
        let condition = column.eq(csv_value);
        self.table.add_condition(condition);
        Ok(())
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
}
