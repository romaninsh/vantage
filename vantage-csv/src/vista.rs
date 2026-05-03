//! Vista bridge for the CSV backend.
//!
//! Construct a `Vista` from a typed `Table<Csv, E>` via `Csv::vista_factory()`,
//! or from a YAML spec via `CsvVistaFactory::from_yaml`. CSV is read-only —
//! all write methods on `VistaSource` return errors.

use std::path::PathBuf;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use vantage_core::{Result, error};
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{Entity, Record};
use vantage_vista::{
    Column as VistaColumn, NoExtras, Vista, VistaCapabilities, VistaFactory, VistaMetadata,
    VistaSource, VistaSpec, flags as vista_flags,
};

use crate::csv::Csv;
use crate::type_system::{AnyCsvType, CsvTypeVariants};

// ---- driver extras --------------------------------------------------------

/// Top-level `csv:` block in a vista spec.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvTableExtras {
    pub csv: CsvBlock,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvBlock {
    pub path: PathBuf,
}

/// Per-column `csv:` block in a vista spec.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvColumnExtras {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csv: Option<CsvColumnBlock>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CsvColumnBlock {
    /// Override the source column header name if it differs from the spec name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

pub type CsvVistaSpec = VistaSpec<CsvTableExtras, CsvColumnExtras, NoExtras>;

// ---- factory --------------------------------------------------------------

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
        let mut variants: IndexMap<String, Option<CsvTypeVariants>> = IndexMap::new();
        for (name, col) in table.columns() {
            let mut vc = VistaColumn::new(name.clone(), col.get_type().to_string());
            if col.flags().contains(&ColumnFlag::Hidden) {
                vc = vc.with_flag(vista_flags::HIDDEN);
            }
            metadata = metadata.with_column(vc);
            variants.insert(name.clone(), variant_from_type_name(col.get_type()));
        }
        if let Some(id_field) = table.id_field() {
            metadata = metadata.with_id_column(id_field.name().to_string());
        }
        for title in table.title_fields() {
            if let Some(col) = metadata.columns.get_mut(title) {
                col.flags.push(vista_flags::TITLE.to_string());
            }
        }

        let table_name = table.table_name().to_string();
        let path = self.csv.file_path_for(&table_name);

        let source = CsvVistaSource {
            path,
            id_column: self.csv.id_column.clone(),
            column_variants: variants,
            capabilities: VistaCapabilities {
                can_count: true,
                ..VistaCapabilities::default()
            },
        };

        Ok(Vista::new(table_name, Box::new(source), metadata))
    }
}

impl VistaFactory for CsvVistaFactory {
    type TableExtras = CsvTableExtras;
    type ColumnExtras = CsvColumnExtras;
    type ReferenceExtras = NoExtras;

    fn build_from_spec(&self, spec: CsvVistaSpec) -> Result<Vista> {
        let path = if spec.driver.csv.path.is_absolute() {
            spec.driver.csv.path.clone()
        } else {
            self.csv.base_dir().join(&spec.driver.csv.path)
        };

        let mut metadata = VistaMetadata::new();
        let mut variants: IndexMap<String, Option<CsvTypeVariants>> = IndexMap::new();
        let mut id_column = spec.id_column.clone();

        for (name, col_spec) in &spec.columns {
            let original_type = col_spec.col_type.clone().unwrap_or_default();
            let mut vc = VistaColumn::new(name.clone(), &original_type);
            for flag in &col_spec.flags {
                vc = vc.with_flag(flag.clone());
                if flag == vista_flags::ID && id_column.is_none() {
                    id_column = Some(name.clone());
                }
            }
            metadata = metadata.with_column(vc);

            let header = col_spec
                .driver
                .csv
                .as_ref()
                .and_then(|b| b.source.clone())
                .unwrap_or_else(|| name.clone());
            variants.insert(header, variant_from_yaml_type(&original_type));
        }

        if let Some(ref id) = id_column {
            metadata = metadata.with_id_column(id.clone());
        }

        let id_for_source = id_column.clone().unwrap_or_else(|| "id".to_string());

        let source = CsvVistaSource {
            path,
            id_column: id_for_source,
            column_variants: variants,
            capabilities: VistaCapabilities {
                can_count: true,
                ..VistaCapabilities::default()
            },
        };

        Ok(Vista::new(spec.name, Box::new(source), metadata))
    }
}

/// Map a YAML type alias (`int`, `string`, ...) to a `CsvTypeVariants`.
fn variant_from_yaml_type(name: &str) -> Option<CsvTypeVariants> {
    match name {
        "int" | "integer" | "i64" | "i32" => Some(CsvTypeVariants::Int),
        "float" | "double" | "f64" | "f32" => Some(CsvTypeVariants::Float),
        "bool" | "boolean" => Some(CsvTypeVariants::Bool),
        "string" | "text" | "str" => Some(CsvTypeVariants::String),
        "json" => Some(CsvTypeVariants::Json),
        _ => None,
    }
}

// Re-use the existing string-type-name lookup for the typed-table path.
use crate::type_system::variant_from_type_name;

// ---- source ---------------------------------------------------------------

/// Per-Vista executor for CSV files.
pub struct CsvVistaSource {
    path: PathBuf,
    id_column: String,
    column_variants: IndexMap<String, Option<CsvTypeVariants>>,
    capabilities: VistaCapabilities,
}

impl CsvVistaSource {
    fn read_filtered(&self, vista: &Vista) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.read_raw()?;
        let out = raw
            .into_iter()
            .filter(|(_, record)| matches_eq_conditions(record, vista))
            .map(|(id, record)| (id, csv_record_to_cbor(record)))
            .collect();
        Ok(out)
    }

    fn read_raw(&self) -> Result<IndexMap<String, Record<AnyCsvType>>> {
        // Borrow a Csv just to reuse the file reader. We pass our own path
        // and id column so the Csv's base_dir doesn't matter here.
        let csv = Csv::new("/").with_id_column(&self.id_column);
        csv.read_csv_with_variants(&self.path, &self.id_column, &self.column_variants)
    }
}

fn csv_record_to_cbor(record: Record<AnyCsvType>) -> Record<CborValue> {
    record.into_iter().map(|(k, v)| (k, v.into())).collect()
}

fn matches_eq_conditions(record: &Record<AnyCsvType>, vista: &Vista) -> bool {
    vista
        .eq_conditions()
        .iter()
        .all(|(field, expected)| match record.get(field) {
            Some(v) => {
                let actual: CborValue = v.clone().into();
                &actual == expected
            }
            None => false,
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
