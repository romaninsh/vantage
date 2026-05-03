//! Vista bridge for the CSV backend.
//!
//! Construct a `Vista` from a typed `Table<Csv, E>` via `Csv::vista_factory()`.
//! At the Vista boundary, CSV's `AnyCsvType` is translated to `ciborium::Value`
//! via the existing CBOR bridge in `type_system.rs`. CSV remains read-only —
//! all write methods on `VistaSource` return errors.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{Entity, Record};
use vantage_vista::{
    Column as VistaColumn, Vista, VistaCapabilities, VistaFactory, VistaMetadata, VistaSource,
};

use crate::csv::Csv;
use crate::type_system::AnyCsvType;

/// Driver-side factory producing `Vista`s backed by CSV files.
pub struct CsvVistaFactory {
    csv: Csv,
}

impl CsvVistaFactory {
    pub fn new(csv: Csv) -> Self {
        Self { csv }
    }

    /// Produce a `Vista` from a typed `Table<Csv, E>`.
    ///
    /// Column metadata is harvested from the typed table; the resulting Vista
    /// owns a `CsvVistaSource` that re-reads the CSV file on each call.
    pub fn from_table<E>(&self, table: Table<Csv, E>) -> Result<Vista>
    where
        E: Entity<AnyCsvType> + 'static,
    {
        let mut metadata = VistaMetadata::new();
        for (name, col) in table.columns() {
            let mut vc = VistaColumn::new(name.clone(), col.get_type().to_string());
            if col.flags().contains(&ColumnFlag::Hidden) {
                vc = vc.hidden();
            }
            metadata = metadata.with_column(vc);
        }
        if let Some(id_field) = table.id_field() {
            metadata = metadata.with_id_column(id_field.name().to_string());
        }
        if !table.title_fields().is_empty() {
            metadata = metadata.with_title_columns(table.title_fields().to_vec());
        }

        let table_name = table.table_name().to_string();
        let columns = table.columns().clone();

        let source = CsvVistaSource {
            csv: self.csv.clone(),
            table_name: table_name.clone(),
            columns,
            capabilities: VistaCapabilities {
                can_count: true,
                ..VistaCapabilities::default()
            },
        };

        Ok(Vista::new(table_name, Box::new(source), metadata))
    }
}

impl VistaFactory for CsvVistaFactory {
    fn from_yaml(&self, _yaml: &str) -> Result<Vista> {
        Err(error!("CSV vista YAML loader not yet implemented"))
    }
}

/// Per-Vista executor for CSV files.
pub struct CsvVistaSource {
    csv: Csv,
    table_name: String,
    columns: IndexMap<String, TableColumn<AnyCsvType>>,
    capabilities: VistaCapabilities,
}

impl CsvVistaSource {
    fn read_filtered(&self, vista: &Vista) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.csv.read_csv(&self.table_name, &self.columns)?;
        let out = raw
            .into_iter()
            .filter(|(_, record)| matches_eq_conditions(record, vista))
            .map(|(id, record)| (id, csv_record_to_cbor(record)))
            .collect();
        Ok(out)
    }
}

fn csv_record_to_cbor(record: Record<AnyCsvType>) -> Record<CborValue> {
    record.into_iter().map(|(k, v)| (k, v.into())).collect()
}

fn matches_eq_conditions(record: &Record<AnyCsvType>, vista: &Vista) -> bool {
    vista.eq_conditions().iter().all(|(field, expected)| {
        match record.get(field) {
            Some(v) => {
                let actual: CborValue = v.clone().into();
                &actual == expected
            }
            None => false,
        }
    })
}

#[async_trait]
impl VistaSource for CsvVistaSource {
    async fn list_vista_values(
        &self,
        vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.read_filtered(vista)
    }

    async fn get_vista_value(
        &self,
        vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let mut data = self.read_filtered(vista)?;
        Ok(data.shift_remove(id))
    }

    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let data = self.read_filtered(vista)?;
        Ok(data.into_iter().next())
    }

    async fn insert_vista_value(
        &self,
        _: &Vista,
        _: &String,
        _: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        Err(error!("CSV is a read-only data source"))
    }

    async fn replace_vista_value(
        &self,
        _: &Vista,
        _: &String,
        _: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        Err(error!("CSV is a read-only data source"))
    }

    async fn patch_vista_value(
        &self,
        _: &Vista,
        _: &String,
        _: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        Err(error!("CSV is a read-only data source"))
    }

    async fn delete_vista_value(&self, _: &Vista, _: &String) -> Result<()> {
        Err(error!("CSV is a read-only data source"))
    }

    async fn delete_vista_all_values(&self, _: &Vista) -> Result<()> {
        Err(error!("CSV is a read-only data source"))
    }

    async fn insert_vista_return_id_value(
        &self,
        _: &Vista,
        _: &Record<CborValue>,
    ) -> Result<String> {
        Err(error!("CSV is a read-only data source"))
    }

    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        Ok(self.read_filtered(vista)?.len() as i64)
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
}

impl Csv {
    /// Return a Vista factory bound to this CSV data source.
    pub fn vista_factory(&self) -> CsvVistaFactory {
        CsvVistaFactory::new(self.clone())
    }
}
