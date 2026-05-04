use std::path::PathBuf;

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

    fn file_path(&self, table_name: &str) -> PathBuf {
        self.base_dir.join(format!("{}.csv", table_name))
    }

    #[cfg(feature = "vista")]
    pub(crate) fn base_dir(&self) -> &std::path::Path {
        &self.base_dir
    }

    /// Parse a CSV file and return all rows as records.
    ///
    /// Behavior under aliasing: a column defined with [`Column::with_alias`]
    /// reads from the CSV header named by its alias and stores under the
    /// column name. Headers without a corresponding column are passed
    /// through untyped (string) under their raw header name — so a table
    /// with zero declared columns still surfaces every field of the file.
    pub(crate) fn read_csv(
        &self,
        table_name: &str,
        columns: &IndexMap<String, Column<AnyCsvType>>,
    ) -> Result<IndexMap<String, Record<AnyCsvType>>> {
        let path = self.file_path(table_name);
        let mut reader = csv::Reader::from_path(&path)
            .map_err(|e| error!("Failed to open CSV file", path = path.display(), detail = e))?;

        let headers = reader
            .headers()
            .map_err(|e| error!("Failed to read CSV headers", detail = e))?
            .clone();

        // For each CSV column index, decide the record key + parsing variant.
        // Default: pass the header through untyped. If a declared column
        // matches (by alias-or-name), use the column name + its type.
        let mut key_for: Vec<String> = headers.iter().map(|h| h.to_string()).collect();
        let mut variant_for: Vec<Option<CsvTypeVariants>> = vec![None; headers.len()];
        for (name, col) in columns {
            let csv_header = col.alias().unwrap_or(name);
            if let Some(idx) = headers.iter().position(|h| h == csv_header) {
                key_for[idx] = name.clone();
                variant_for[idx] = variant_from_type_name(col.get_type());
            }
        }

        // id column may itself carry an alias.
        let id_header = columns
            .get(&self.id_column)
            .and_then(|c| c.alias())
            .unwrap_or(&self.id_column);
        let id_col_index = headers.iter().position(|h| h == id_header);

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
                if let Some(key) = key_for.get(i) {
                    let variant = variant_for.get(i).copied().flatten();
                    record.insert(key.clone(), parse_with_type(field, variant));
                }
            }

            records.insert(id, record);
        }

        Ok(records)
    }
}
