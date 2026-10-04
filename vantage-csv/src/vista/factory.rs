//! `CsvVistaFactory` — typed-table and YAML entry points, plus the
//! `VistaFactory` trait impl. CSV is read-only, so the factory advertises
//! only `can_count`.

use vantage_core::{Result, error};
use vantage_table::column::core::Column as TableColumn;
use vantage_table::table::{Table, VistaMetadataOptions};
use vantage_types::{EmptyEntity, Entity};
use vantage_vista::{NoExtras, Vista, VistaCapabilities, VistaFactory};

use crate::csv::Csv;
use crate::type_system::AnyCsvType;
use crate::vista::source::CsvTableShell;
use crate::vista::spec::{CsvColumnExtras, CsvTableExtras, CsvVistaSpec};

pub struct CsvVistaFactory {
    csv: Csv,
}

impl CsvVistaFactory {
    pub fn new(csv: Csv) -> Self {
        Self { csv }
    }

    /// Wrap a typed table as a Vista. Column metadata is harvested from the
    /// table; CRUD goes through `Table`'s reading path.
    pub fn from_table<E>(&self, table: Table<Csv, E>) -> Result<Vista>
    where
        E: Entity<AnyCsvType> + 'static,
    {
        let metadata = table.vista_metadata(VistaMetadataOptions::default());
        let name = table.table_name().to_string();
        let any_table = table.into_entity::<EmptyEntity>();

        let source = CsvTableShell::new(
            any_table,
            VistaCapabilities {
                can_count: true,
                can_traverse_to_record: true,
                ..VistaCapabilities::default()
            },
            metadata,
        );
        Ok(Vista::new(name, Box::new(source)))
    }

    /// Build a `Table<Csv, EmptyEntity>` from a spec. Each column is added
    /// with its YAML-declared type; `csv.source` becomes the column's alias
    /// so `read_csv` knows which CSV header to read from.
    pub fn table_from_spec(&self, spec: &CsvVistaSpec) -> Result<Table<Csv, EmptyEntity>> {
        let csv_path = if spec.driver.csv.path.is_absolute() {
            spec.driver.csv.path.clone()
        } else {
            self.csv.base_dir().join(&spec.driver.csv.path)
        };
        let stem = csv_path
            .file_stem()
            .ok_or_else(|| error!("csv.path must point to a file", path = csv_path.display()))?
            .to_string_lossy()
            .to_string();
        let parent = csv_path
            .parent()
            .ok_or_else(|| error!("csv.path must have a parent", path = csv_path.display()))?
            .to_path_buf();

        let id_column = spec.resolve_id_column();
        let csv = Csv::new(parent).with_id_column(&id_column);
        let mut table = Table::<Csv, EmptyEntity>::new(stem, csv);
        table.add_spec_columns(&spec.columns, build_column)?;
        table.set_spec_id_field(&id_column)?;
        Ok(table)
    }
}

impl VistaFactory for CsvVistaFactory {
    type TableExtras = CsvTableExtras;
    type ColumnExtras = CsvColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: CsvVistaSpec) -> Result<Vista> {
        let vista_name = spec.name.clone();
        let table = self.table_from_spec(&spec)?;
        let mut vista = self.from_table(table)?;
        vista.set_name(vista_name);
        Ok(vista)
    }
}

/// The `csv.source` block names the CSV header to read.
pub(crate) fn build_column(
    name: &str,
    col_spec: &vantage_vista::ColumnSpec<CsvColumnExtras>,
) -> Result<TableColumn<AnyCsvType>> {
    let header = col_spec.driver.csv.as_ref().and_then(|b| b.source.clone());
    TableColumn::from_spec(name, col_spec, header, column_for_type)
}

/// Runtime type dispatch — YAML type alias to `Column<T>` (then erased to
/// `Column<AnyCsvType>` for storage). New aliases should land in both
/// `column_for_type` and the typed-table convenience macros.
pub(crate) fn column_for_type(name: &str, ty: &str) -> Result<TableColumn<AnyCsvType>> {
    let col: TableColumn<AnyCsvType> = match ty {
        "int" | "integer" | "i64" | "i32" => {
            TableColumn::from_column(TableColumn::<i64>::new(name))
        }
        "float" | "double" | "f64" | "f32" => {
            TableColumn::from_column(TableColumn::<f64>::new(name))
        }
        "bool" | "boolean" => TableColumn::from_column(TableColumn::<bool>::new(name)),
        "string" | "text" | "str" => TableColumn::from_column(TableColumn::<String>::new(name)),
        "json" => TableColumn::from_column(TableColumn::<serde_json::Value>::new(name)),
        other => {
            return Err(error!(
                "Unknown YAML column type",
                column = name,
                ty = other.to_string()
            ));
        }
    };
    Ok(col)
}
