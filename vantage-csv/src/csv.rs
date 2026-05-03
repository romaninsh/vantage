use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_table::column::core::Column;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::Record;

use crate::type_system::{AnyCsvType, CsvTypeVariants, parse_with_type, variant_from_type_name};

/// CSV backend for Vantage — reads data from CSV files.
///
/// Each table maps to a CSV file: `{base_dir}/{table_name}.csv`.
/// CSV is a read-only data source — write operations return errors.
#[derive(Clone, Debug)]
pub struct Csv {
    base_dir: PathBuf,
    pub(crate) id_column: String,
}

impl Csv {
    /// Create a new CSV data source reading files from `base_dir`.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            id_column: "id".to_string(),
        }
    }

    /// Set which CSV column to use as the record ID.
    pub fn with_id_column(mut self, column: impl Into<String>) -> Self {
        self.id_column = column.into();
        self
    }

    /// Get the file path for a given table name.
    fn file_path(&self, table_name: &str) -> PathBuf {
        self.file_path_for(table_name)
    }

    pub(crate) fn file_path_for(&self, table_name: &str) -> PathBuf {
        self.base_dir.join(format!("{}.csv", table_name))
    }

    pub(crate) fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Parse a CSV file and return all rows as records.
    ///
    /// Column types from the table definition are used to parse values.
    /// Fields without a known column type are stored as plain strings.
    pub(crate) fn read_csv(
        &self,
        table_name: &str,
        columns: &IndexMap<String, Column<AnyCsvType>>,
    ) -> Result<IndexMap<String, Record<AnyCsvType>>> {
        let mut variants = IndexMap::new();
        for (name, col) in columns {
            variants.insert(name.clone(), variant_from_type_name(col.get_type()));
        }
        self.read_csv_with_variants(&self.file_path(table_name), &self.id_column, &variants)
    }

    /// Parse a CSV file using pre-resolved column type variants.
    ///
    /// Used by the Vista bridge, where column types come from a `VistaSpec`
    /// rather than a typed `Table<E>`.
    pub(crate) fn read_csv_with_variants(
        &self,
        path: &Path,
        id_column: &str,
        column_variants: &IndexMap<String, Option<CsvTypeVariants>>,
    ) -> Result<IndexMap<String, Record<AnyCsvType>>> {
        let mut reader = csv::Reader::from_path(path)
            .map_err(|e| error!("Failed to open CSV file", path = path.display(), detail = e))?;

        let headers = reader
            .headers()
            .map_err(|e| error!("Failed to read CSV headers", detail = e))?
            .clone();

        let id_col_index = headers.iter().position(|h| h == id_column);

        let mut records = IndexMap::new();

        for (row_idx, result) in reader.records().enumerate() {
            let csv_record = result.map_err(|e| error!("Failed to read CSV row", detail = e))?;

            let id = if let Some(idx) = id_col_index {
                csv_record.get(idx).unwrap_or_default().to_string()
            } else {
                row_idx.to_string()
            };

            let mut record = Record::new();
            for (i, field) in csv_record.iter().enumerate() {
                if let Some(header) = headers.get(i) {
                    let variant = column_variants.get(header).copied().flatten();
                    record.insert(header.to_string(), parse_with_type(field, variant));
                }
            }

            records.insert(id, record);
        }

        Ok(records)
    }
}
